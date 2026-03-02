// src/agent.rs — v0.4: State Machine

use anyhow::Result;
use std::path::PathBuf;
use crate::{
    executor::SafeExecutor,
    llm::LlmClient,
    protocol::{self, Cmd},
    types::{AgentState, ExecutionContext, FailedStep, FailureKind, Message},
};

pub struct Agent {
    state:     AgentState,
    ctx:       ExecutionContext,
    executor:  SafeExecutor,
    llm:       LlmClient,
    goal:      String,
    plan:      Vec<Cmd>,
}

impl Agent {
    pub fn new(api_key: String, workspace: PathBuf, goal: String, max_repairs: u8) -> Self {
        Self {
            state:    AgentState::Planning,
            ctx:      ExecutionContext::new(max_repairs),
            executor: SafeExecutor::new(workspace, 120),
            llm:      LlmClient::new(api_key),
            goal,
            plan:     Vec::new(),
        }
    }

    // ══════════════════════════════════════════════════════════
    // الحلقة الرئيسية
    // ══════════════════════════════════════════════════════════

    pub async fn run(&mut self) -> Result<()> {
        // تحميل الـ hashes من الجلسة السابقة
        let ws = self.executor.workspace.clone();
        self.ctx.load_hashes(&ws);
        let loaded = self.ctx.successful_hashes.len();
        if loaded > 0 {
            println!("   💾 Loaded {} cached steps from previous session", loaded);
        }
        loop {
            match self.state.clone() {

                // ─── Planning ─────────────────────────────────
                AgentState::Planning => {
                    println!("\n🧠 Planning...");
                    let prompt = format!(
                        "Goal: {}\n\nProvide the complete execution plan.",
                        self.goal
                    );
                    let response = self.llm.call(&[Message::user(prompt)]).await?;
                    match protocol::parse(&response) {
                        Ok(plan) => {
                            println!("   ✓ {} commands\n", plan.commands.len());
                            self.plan  = plan.commands;
                            self.state = AgentState::Executing;
                        }
                        Err(e) => {
                            println!("   ❌ Invalid plan: {}", e);
                            self.state = AgentState::Failed(e.to_string());
                        }
                    }
                }

                // ─── Executing ────────────────────────────────
                // تنفّذ كل الأوامر بدون LLM
                // تجمع الأخطاء — لا تتوقف عند أول فشل
                AgentState::Executing => {
                    self.ctx.reset_for_repair();
                    let plan = self.plan.clone();
                    let total = plan.len();

                    for (i, cmd) in plan.iter().enumerate() {
                        println!("[{}/{}] {}", i + 1, total, cmd.label());

                        // skip الخطوات الناجحة سابقاً
                        let cmd_hash = cmd.hash();
                        if self.ctx.successful_hashes.contains(&cmd_hash) && !cmd.is_run_tests() && !cmd.is_write_file() {
                            println!("   ⏭ Skipping: {} (already passed)", cmd.label());
                            continue;
                        }

                        // done مشروط — لا يُنفَّذ إذا لم تنجح الاختبارات
                        if cmd.is_done() {
                            if self.ctx.tests_passed {
                                let msg = if let Cmd::Done { message } = cmd { message } else { "Goal complete" };
                                self.ctx.save_hashes(&self.executor.workspace);
                                println!("\n✅ {}", if msg.is_empty() { "Goal complete!" } else { msg });
                                self.state = AgentState::Done;
                            } else {
                                println!("   ⛔ done rejected — tests must pass first");
                                self.ctx.failed_steps.push(FailedStep {
                                    step_index: i,
                                    label:      cmd.label(),
                                    stderr:     "done blocked: tests_passed = false".into(),
                                    exit_code:  1,
                                });
                                self.state = AgentState::Repairing;
                            }
                            break;
                        }

                        // تنفيذ الأمر
                        match self.executor.run(cmd).await {
                            Ok(r) if r.success => {
                                let preview: String = r.stdout.chars().take(80).collect();
                                if preview.is_empty() {
                                    println!("   ✓ ({} ms)", r.duration_ms);
                                } else {
                                    println!("   ✓ ({} ms) → {}", r.duration_ms, preview);
                                }
                                // تسجيل نجاح الاختبارات
                                if cmd.is_run_tests() { self.ctx.tests_passed = true; }
                                self.ctx.successful_hashes.insert(cmd_hash.clone());
                            }
                            Ok(r) => {
                                let err: String = r.stderr.chars().take(3000).collect();
                                println!("   ✗ {}", err);
                                // تسجيل الفشل — تابع بقية الأوامر
                                if cmd.is_run_tests() { self.ctx.tests_passed = false; }
                                self.ctx.failed_steps.push(FailedStep {
                                    step_index: i,
                                    label:      cmd.label(),
                                    stderr:     err,
                                    exit_code:  r.exit_code,
                                });
                            }
                            Err(e) => {
                                println!("   ❌ {}", e);
                                self.ctx.failed_steps.push(FailedStep {
                                    step_index: i,
                                    label:      cmd.label(),
                                    stderr:     e.to_string(),
                                    exit_code:  -1,
                                });
                            }
                        }
                    }

                    // بعد كل الأوامر — قرر الحالة التالية
                    if matches!(self.state, AgentState::Executing) {
                        if self.ctx.tests_passed {
                            self.ctx.save_hashes(&self.executor.workspace);
                            println!("\n✅ Goal complete! Tests passed.");
                            self.state = AgentState::Done;
                        } else {
                            self.state = AgentState::Repairing;
                        }
                    }
                }

                // ─── Repairing ────────────────────────────────
                // استدعاء LLM واحد لخطة إصلاح
                AgentState::Repairing => {
                    self.ctx.repair_attempts += 1;

                    if self.ctx.repair_attempts > self.ctx.max_repairs {
                        let reason = format!(
                            "Failed after {} repair attempts. Last errors:\n{}",
                            self.ctx.repair_attempts - 1,
                            self.ctx.failed_steps.iter()
                                .map(|f| format!("  • {}: {}", f.label, { let s = &f.stderr; let start = s.len().saturating_sub(1000); &s[start..] }))
                                .collect::<Vec<_>>()
                                .join("\n")
                        );
                        self.state = AgentState::Failed(reason);
                        continue;
                    }

                    println!("\n🔧 Repair {}/{}...", self.ctx.repair_attempts, self.ctx.max_repairs);

                    let ws = &self.executor.workspace;
                    // Dynamic file discovery — يقرأ كل الملفات التي كُتبت في الـ plan
                    let written_files: Vec<String> = self.plan.iter()
                        .filter_map(|c| match c {
                            crate::protocol::Cmd::WriteFile { path, .. } => Some(path.clone()),
                            _ => None,
                        })
                        .collect();
                    let files_context: String = written_files.iter()
                        .filter_map(|f| {
                            let content = std::fs::read_to_string(ws.join(f)).ok()?;
                            if content.is_empty() { return None; }
                            let lang = if f.ends_with(".py") { "python" }
                                       else if f.ends_with(".rs") { "rust" }
                                       else if f.ends_with(".js") { "javascript" }
                                       else if f.ends_with(".go") { "go" }
                                       else if f.ends_with(".toml") { "toml" }
                                       else { "text" };
                            Some(format!("{}:\n```{}\n{}\n```", f, lang, content))
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    // backward compat
                    let main_py = String::new();
                    let test_py = String::new();
                    let _ = (main_py.as_str(), test_py.as_str());

                    let errors = self.ctx.failed_steps.iter()
                        .map(|f| format!("Step '{}' failed (exit {}):\n{}", f.label, f.exit_code, { let s = &f.stderr; let start = s.len().saturating_sub(2000); &s[start..] }))
                        .collect::<Vec<_>>()
                        .join("\n");

                    // تصنيف نوع الفشل
                    let all_stderr = self.ctx.failed_steps.iter()
                        .map(|f| f.stderr.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let failure_kind = FailureKind::classify(&all_stderr);
                    let repair_hint = failure_kind.repair_hint();
                    println!("   🔍 Failure type: {:?}", failure_kind);

                    let network_note = if errors.contains("Network is unreachable") || errors.contains("Timeout after") {
                        "\n\nNETWORK UNAVAILABLE: Use ONLY Python stdlib. NO pandas, NO requests."
                    } else { "" };

                    let attempt_note = if self.ctx.repair_attempts > 1 {
                        format!("ATTEMPT {}/{}: Previous fix failed — try a completely different approach.", self.ctx.repair_attempts, self.ctx.max_repairs)
                    } else {
                        format!("ATTEMPT {}/{}: First repair attempt.", self.ctx.repair_attempts, self.ctx.max_repairs)
                    };
                    let prompt = format!(
                        "Goal: {}{}\n\nHINT: {}\n\n{}\n\nFAILED STEPS:\n{}\n\nCURRENT FILES:\n{}\n\
                         Fix ALL issues. Provide complete corrected plan.",
                        self.goal, network_note, repair_hint, attempt_note, errors, files_context
                    );

                    match self.llm.call(&[Message::user(prompt)]).await {
                        Ok(response) => {
                            match protocol::parse(&response) {
                                Ok(plan) => {
                                    println!("   ✓ Repair plan: {} commands", plan.commands.len());
                                    self.plan  = plan.commands;
                                    self.state = AgentState::Executing;
                                }
                                Err(e) => {
                                    println!("   ⚠ Invalid repair plan: {}", e);
                                    // أعد المحاولة في دورة Repairing التالية
                                }
                            }
                        }
                        Err(e) => {
                            println!("   ⚠ LLM error: {}", e);
                        }
                    }
                }

                // ─── Terminal States ───────────────────────────
                AgentState::Done => return Ok(()),
                AgentState::Failed(reason) => {
                    println!("\n❌ Agent failed: {}", reason);
                    return Ok(());
                }
            }
        }
    }
}
