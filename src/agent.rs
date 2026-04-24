// src/agent.rs — v1.3: State Machine

use crate::{
    executor::SafeExecutor,
    llm_engine::LlmEngine,
    protocol::Cmd,
    types::{AgentState, ContextConfig, ExecutionContext, FailedStep, FailureKind, Message},
};
use anyhow::Result;
use std::path::PathBuf;

pub struct Agent {
    state: AgentState,
    pub ctx: ExecutionContext,
    executor: SafeExecutor,
    llm: LlmEngine,
    goal: String,
    plan: Vec<Cmd>,
    previous_error: Option<String>,
    repair_fingerprints: Vec<u64>, // Repair History Guard v1.2
    context_config: ContextConfig,
    failure_memory: crate::memory::FailureMemory, // v5.8
    pub accumulated_stats: crate::llm_engine::LlmCallStats, // v6.1
    initial_snapshot: Option<crate::snapshot::Snapshot>, // v7.4.1
}

impl Agent {
    pub fn new(
        _api_key: String,
        workspace: PathBuf,
        goal: String,
        max_repairs: u8,
        context_config: ContextConfig,
    ) -> Self {
        Self {
            state: AgentState::Planning,
            ctx: ExecutionContext::new(max_repairs),
            executor: SafeExecutor::new(workspace, 120),
            llm: crate::llm_engine::LlmEngine::from_env(),
            goal,
            plan: Vec::new(),
            previous_error: None,
            repair_fingerprints: Vec::new(),
            context_config,
            failure_memory: crate::memory::FailureMemory::load(),
            accumulated_stats: crate::llm_engine::LlmCallStats::default(),
            initial_snapshot: None,
        }
    }
    pub fn new_with_model(
        api_key: String,
        _model_alias: String,
        workspace: PathBuf,
        goal: String,
        max_repairs: u8,
        context_config: ContextConfig,
    ) -> Self {
        Self::new(api_key, workspace, goal, max_repairs, context_config)
    }

    pub fn call_stats(&self) -> &crate::llm_engine::LlmCallStats {
        &self.accumulated_stats
    }

    pub fn repair_count(&self) -> usize {
        self.ctx.repair_attempts as usize
    }

    pub fn is_success(&self) -> bool {
        matches!(self.state, AgentState::Done)
    }
    fn send_event(
        &self,
        event_type: &str,
        step: Option<&str>,
        detail: Option<&str>,
        success: Option<bool>,
        mutation: Option<f64>,
    ) {
        let body = serde_json::json!({
            "event_type": event_type,
            "goal": &self.goal,
            "step": step.unwrap_or(""),
            "detail": detail.unwrap_or(""),
            "success": success,
            "mutation": mutation,
            "repairs": self.ctx.repair_attempts,
            "model": "auto",
            "timestamp": ""
        });
        let url = std::env::var("SEL_OBSERVATORY")
            .unwrap_or_else(|_| "http://localhost:8777".to_string());
        let url = format!("{}/api/event", url);
        // Fire-and-forget: non-blocking send via tokio::spawn
        tokio::spawn(async move {
            let _ = reqwest::Client::new()
                .post(&url)
                .json(&body)
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await;
        });
    }

    pub fn mutation_score(&self) -> f64 {
        if self.ctx.mutations_total > 0 {
            self.ctx.mutations_killed as f64 / self.ctx.mutations_total as f64
        } else {
            -1.0
        }
    }

    // ══════════════════════════════════════════════════════════
    // Goal Validator — delegated to decision module
    // ══════════════════════════════════════════════════════════
    
    // v1.3: Unified Mutation Check Helper
    fn is_impl_file(path: &str) -> bool {
        let p = path.to_lowercase();
        let ext_ok = p.ends_with(".py") || p.ends_with(".go") || p.ends_with(".js") || p.ends_with(".ts") || p.ends_with(".rs");
        if !ext_ok { return false; }
        
        let is_config = p.contains("cargo.toml") || p.contains("go.mod") || p.contains("package.json") || p.contains("tsconfig") || p.contains("jest.config");
        let is_test = p.contains("test") || p.contains("spec");
        
        !is_config && !is_test
    }
    
    async fn run_mutation_check(&mut self) -> Option<crate::types::FailedStep> {
        if self.ctx.skip_mutation { return None; }
        
        let impl_source = self.plan.iter().find_map(|c| match c {
            crate::protocol::Cmd::WriteFile { path, .. } if Self::is_impl_file(path) => Some(path.clone()),
            crate::protocol::Cmd::PatchFile { path, .. } if Self::is_impl_file(path) => Some(path.clone()),
            _ => None,
        });
        
        if let Some(src) = impl_source {
            use crate::executor::MutationResult;
            println!("\n🧬 Mutation check on {}...", src);
            match self.executor.mutation_check(&src).await {
                MutationResult::Weak(orig_line, mutd_line) => {
                    self.ctx.mutations_total += 1;
                    println!("   ⚠️  Tests are WEAK — triggering repair (Mutation Enforcement v1.3).");
                    println!("     Survived mutation: [{}] → [{}]", orig_line, mutd_line);
                    
                    Some(crate::types::FailedStep {
                        step_index:   0,
                        label:        "mutation_check".into(),
                        stderr:       format!(
                            "WEAK TESTS: Tests passed on mutated code in '{}'.
                             SURVIVED MUTATION DIFF:
                             - Original : {}
                             + Mutated  : {}
                             The tests did NOT catch this change.
                             Add assertions that distinguish these two behaviors.",
                            src, orig_line, mutd_line
                        ),
                        exit_code:    -3,
                        culprit_file: Some(src),
                    })
                }
                MutationResult::Strong => {
                    println!("   ✅ Tests are solid.");
                    self.send_event("mutation", None, None, None, Some(1.0));
                    self.ctx.mutations_total += 1;
                    self.ctx.mutations_killed += 1;
                    None
                }
                MutationResult::Skipped => {
                    println!("   ⏭  Mutation check skipped.");
                    None
                }
            }
        } else {
            None
        }
    }

    // ══════════════════════════════════════════════════════════

    // ══════════════════════════════════════════════════════════
    // Protocol Resilience v1.3
    async fn plan_with_resilience(&mut self, base_prompt: String) -> Result<Vec<Cmd>, String> {
        const MAX_RETRIES: u8 = 2;
        for attempt in 0..=MAX_RETRIES {
            let prompt = if attempt == 0 {
                base_prompt.clone()
            } else {
                format!(
                    "{}\n\n                     WARNING — PROTOCOL RETRY {}/{}: previous response could not be parsed.\n                     STRICT RULES:\n                     1. Output ONLY a ```json block — zero text outside it.\n                     2. Keep all content strings SHORT (< 40 chars per line).\n                     3. Use ONLY single quotes inside Python/shell code.\n                     4. No raw newlines inside JSON strings — use \\n instead.\n                     5. No special characters that break JSON strings.",
                    base_prompt, attempt, MAX_RETRIES
                )
            };

            match self.llm.call(&[Message::user(prompt)]).await {
                Ok(response) => {
                    self.accumulated_stats.retries += 1;
                    
                    
                    
                    self.accumulated_stats.total_latency_ms += self.llm.stats.total_latency_ms;
                    match crate::protocol::parse(&response) {
                        Ok(plan) => {
                            if attempt > 0 {
                                println!("   ✅ Protocol retry {} succeeded.", attempt);
                            }
                            return Ok(plan.commands);
                        }
                        Err(e) => {
                            if attempt < MAX_RETRIES {
                                let err_msg = e.to_string();
                                println!(
                                    "   {} (attempt {}/{})",
                                    crate::llm_engine::classify_json_error(&err_msg),
                                    attempt + 1,
                                    MAX_RETRIES + 1
                                );
                            } else {
                                return Err(format!(
                                    "JSON parse failed after {} attempts: {}",
                                    MAX_RETRIES + 1,
                                    e
                                ));
                            }
                        }
                    }
                }
                Err(e) => {
                    // v6.1: تسجيل أخطاء الاتصال حتى عند فشل call()
                    let msg = e.to_string();
                    if msg.contains("Connection error") {
                        self.accumulated_stats.connection_errors += 1;
                    } else if msg.contains("Rate limit") || msg.contains("429") {
                        self.accumulated_stats.rate_limits += 1;
                    } else if msg.contains("503") || msg.contains("502") || msg.contains("500") {
                        self.accumulated_stats.connection_errors += 1;
                    }
                    self.accumulated_stats.retries += attempt as u32;
                    return Err(msg);
                }
            }
        }
        unreachable!()
    }
    // ══════════════════════════════════════════════════════════
    // v5.6: Context & validation — delegated to decision module
    // ══════════════════════════════════════════════════════════





    fn validate_plan_with_oracle(&self, plan: &[Cmd]) -> Vec<String> {
        let mut issues = Vec::new();
        let mut has_test = false;

        for cmd in plan {
            if cmd.is_run_tests() {
                has_test = true;
            }
            if let Err(e) = self.executor.oracle.validate_plan_cmd(cmd) {
                issues.push(e);
            }
        }

        if !has_test {
            issues.push("PLAN ERROR: Every plan MUST include at least one run_tests command to verify the changes.".to_string());
        }

        issues
    }

    async fn replan_with_feedback(
        &mut self,
        original_plan: Vec<Cmd>,
        issues: Vec<String>,
    ) -> Result<Vec<Cmd>, String> {
        self.ctx.replan_attempts += 1;
        if self.ctx.replan_attempts > 2 {
            println!("   ⚠ Max replan attempts (2) reached — proceeding with original plan");
            return Ok(original_plan);
        }
        println!(
            "\n   🔄 v5.6 Replan {}/2 — patch uniqueness issues:",
            self.ctx.replan_attempts
        );
        for issue in &issues {
            println!("      • {}", issue);
        }
        let feedback = format!(
            "PLAN REJECTED — patch_file uniqueness issues:\n{}\n\n\
             MANDATORY RULES:\n\
             1. Each patch_file search block must appear EXACTLY ONCE in the target file.\n\
             2. If search block not found → file does not exist yet, use write_file instead.\n\
             3. If found multiple times → add more surrounding context lines to make it unique.\n\
             4. Copy search text VERBATIM from the file (case-sensitive, exact whitespace).\n\n\
             Provide corrected execution plan.",
            issues.join("\n")
        );
        let ws = &self.executor.workspace;
        let existing_files = if self.context_config.ref_file.is_some() {
            crate::decision::build_skeleton_context(ws)
        } else {
            String::new()
        };
        let ref_context = crate::decision::build_ref_context(&self.context_config);
        let lang_hint = crate::decision::build_lang_hint(ws);
        let prompt = crate::constitution::CONSTITUTION.to_string() + &format!(
            "{}{}{}\nGoal: {}\n\nFEEDBACK:\n{}",
            existing_files, ref_context, lang_hint, self.goal, feedback
        );
        match self.plan_with_resilience(prompt).await {
            Ok(new_plan) => {
                let new_issues = crate::decision::validate_patch_uniqueness(&self.executor.workspace, &new_plan);
                if new_issues.is_empty() {
                    println!("   ✅ v5.6 Replan successful — all patches unique");
                    Ok(new_plan)
                } else {
                    Box::pin(self.replan_with_feedback(new_plan, new_issues)).await
                }
            }
            Err(e) => Err(e),
        }
    }
    // ══════════════════════════════════════════════════════════

    pub async fn run(&mut self) -> Result<()> {
        self.ctx.start_time = Some(std::time::Instant::now());
        // v7.4.1: Take initial snapshot to allow final rollback on complete failure
        self.initial_snapshot = Some(crate::snapshot::Snapshot::take(&self.executor.workspace));

        self.send_event("start", None, None, None, None);
        // v5.8.1: امسح الـ cache في بداية كل run — كل جلسة تبدأ نظيفة
        let ws = self.executor.workspace.clone();
        let cache_path = ws.join(".sel_hashes");
        if cache_path.exists() {
            let _ = std::fs::remove_file(&cache_path);
            println!("   🗑  Cache cleared — fresh start");
        }

        // v6.3: ScaffoldEngine — يُجهّز البيئة قبل LLM
        let scaffold = crate::scaffold_engine::prepare(&ws, &self.goal).await;
        if scaffold.ready {
            println!(
                "   🏗  Scaffold ready: {:?} ({} files)",
                scaffold.kind,
                scaffold.files_created.len()
            );
            if !scaffold.logic_hint.is_empty() {
                self.goal = format!(
                    "{}
{}",
                    self.goal, scaffold.logic_hint
                );
            }
        }

        loop {
            match self.state.clone() {
                // ─── Planning ─────────────────────────────────
                AgentState::Planning => {
                    // Goal Validator v1.2 — delegated to decision module
                    if let Some(reason) = crate::decision::validate_goal(&self.goal) {
                        println!("\n❌ Invalid goal: {}", reason);
                        println!("SEL_FAILED: {}", reason);
                        self.state = AgentState::Failed(reason.to_string());
                        break Ok(());
                    }
                    println!("\n🧠 Planning...");
                    self.send_event(
                        "step",
                        Some("Planning"),
                        Some("Generating execution plan"),
                        None,
                        None,
                    );
                    // v6.1: ECM — probe البيئة قبل Planning
                    let ecm = crate::environment::EnvironmentCapabilities::probe();
                    println!("   🔍 Environment: {}", {
                        let py = ecm
                            .python
                            .as_ref()
                            .map(|p| format!("python={}", p.cmd))
                            .unwrap_or("python=none".into());
                        let nd = if ecm.node.is_some() {
                            "node=✓"
                        } else {
                            "node=✗"
                        };
                        let rs = if ecm.rust.is_some() {
                            "rust=✓"
                        } else {
                            "rust=✗"
                        };
                        format!("{} {} {}", py, nd, rs)
                    });
                    let env_context = ecm.to_planning_context();
                    let constraints = ecm.derive_constraints();
                    // v5.6: delegated to decision module
                    let ws = &self.executor.workspace;
                    let lang_hint = crate::decision::build_lang_hint(ws);
                    // v6.6: Auto-Context Injection
                    let ws_ctx = crate::decision::build_workspace_context(ws);
                    let existing_files = if !ws_ctx.is_empty() {
                        ws_ctx
                    } else if self.context_config.ref_file.is_some() {
                        crate::decision::build_skeleton_context(ws)
                    } else {
                        String::new()
                    };

                    let ref_context = crate::decision::build_ref_context(&self.context_config);

                    let prompt = crate::constitution::CONSTITUTION.to_string() + &format!(
                        "{}{}{}\n{}\n{}\nGoal: {}\nProvide the complete execution plan.",
                        existing_files, ref_context, lang_hint, env_context, constraints, self.goal
                    );
                    // Protocol Resilience v1.3
                    match self.plan_with_resilience(prompt).await {
                        Ok(commands) => {
                            // v7.4: Constraint Engine — filter invalid commands
                            let commands_count = commands.len();
                            let env = crate::constraint_engine::ProjectEnv::detect(&self.executor.workspace);
                            let state = crate::constraint_engine::ProjectState::scan(&self.executor.workspace);
                            let commands = match crate::constraint_engine::apply(commands, &env, &state) {
                                crate::constraint_engine::ConstraintResult::Ok(filtered) => {
                                    if filtered.len() < commands_count {
                                        println!("   🛡️ Constraint Engine: filtered {} commands",
                                            commands_count - filtered.len());
                                    }
                                    filtered
                                }
                                crate::constraint_engine::ConstraintResult::Fatal(reason) => {
                                    println!("   ⛔ Constraint Engine: {}", reason);
                                    self.state = AgentState::Failed(reason);
                                    continue;
                                }
                            };
                            println!("   ✓ {} commands", commands.len());
                            // 🛡️ Pre-Execution Oracle Validation
                            let mut issues = self.validate_plan_with_oracle(&commands);
                            
                            // v5.6: Unique Patch Enforcer — delegated to decision module
                            let patch_issues = crate::decision::validate_patch_uniqueness(&self.executor.workspace, &commands);
                            issues.extend(patch_issues);

                            if !issues.is_empty() {
                                match self.replan_with_feedback(commands, issues).await {
                                    Ok(valid_commands) => {
                                        println!(
                                            "   ✓ Final plan: {} commands\n",
                                            valid_commands.len()
                                        );
                                        self.plan = valid_commands;
                                        self.state = AgentState::Executing;
                                    }
                                    Err(e) => {
                                        println!("   ❌ Replan failed: {}", e);
                                        self.state = AgentState::Failed(e);
                                    }
                                }
                            } else {
                                println!("   ✓ All patches unique\n");
                                self.plan = commands;
                                self.state = AgentState::Executing;
                            }
                        }
                        Err(e) => {
                            println!("   ❌ Plan parse failed: {}", e);
                            self.state = AgentState::Failed(e);
                        }
                    }
                }

                // ─── Executing ────────────────────────────────
                // تنفّذ كل الأوامر بدون LLM
                // تجمع الأخطاء — لا تتوقف عند أول فشل
                AgentState::Executing => {
                    self.ctx.reset_for_repair();
                    // v7.4: Snapshot — save workspace state before execution
                    let mut snapshot = crate::snapshot::Snapshot::take(&self.executor.workspace);
                    let plan = self.plan.clone();
                    let total = plan.len();

                    for (i, cmd) in plan.iter().enumerate() {
                        println!("[{}/{}] {}", i + 1, total, cmd.label());
                        {
                            let _lbl = cmd.label();
                            let _prog = format!("{}/{}", i + 1, total);
                            self.send_event("step", Some(&_lbl), Some(&_prog), None, None);
                        }

                        // skip الخطوات الناجحة سابقاً
                        let cmd_hash = cmd.hash();
                        // pip install يُعاد تشغيله إذا كان venv غير موجود (مثلاً بعد rm -rf venv)
                        let is_pip = cmd.label().contains("pip");
                        let venv_ok = self.executor.workspace.join("venv/bin/pip3").exists()
                            || self.executor.workspace.join("venv/bin/pip").exists();
                        // منع cargo test/check من الـ cache — يجب إعادة تنفيذها دائماً
                        let is_cargo_test = cmd.label().contains("cargo test")
                            || cmd.label().contains("cargo check");
                        let skip_allowed = !cmd.is_run_tests()
                            && !cmd.is_write_file()
                            && !cmd.is_patch_file()
                            && !is_cargo_test
                            && !(is_pip && !venv_ok);
                        if self.ctx.successful_hashes.contains(&cmd_hash) && skip_allowed {
                            println!("   ⏭ Skipping: {} (already passed)", cmd.label());
                            continue;
                        }
                        if is_pip && !venv_ok {
                            println!("   🔄 venv missing — re-running pip install");
                        }

                        // done مشروط — لا يُنفَّذ إذا لم تنجح الاختبارات
                        if cmd.is_done() {
                            if self.ctx.tests_passed {
                                let msg = if let Cmd::Done { message } = cmd {
                                    message
                                } else {
                                    "Goal complete"
                                };
                                // ─── Mutation Check v1.3 ───
                                if let Some(fail) = self.run_mutation_check().await {
                                    self.ctx.failed_steps.push(fail);
                                    self.state = AgentState::Repairing;
                                } else {
                                    self.ctx.save_hashes(&self.executor.workspace);
                                    println!(
                                        "\n✅ {}",
                                        if msg.is_empty() {
                                            "Goal complete!"
                                        } else {
                                            msg
                                        }
                                    );
                                    println!("SEL_SUCCESS");
                                    self.send_event(
                                        "done",
                                        None,
                                        None,
                                        Some(true),
                                        Some(self.mutation_score()),
                                    );
                                    self.state = AgentState::Done;
                                }
                                // ─────────────────────────────
                            } else {
                                println!("   ⛔ done rejected — tests must pass first");
                                self.ctx.failed_steps.push(FailedStep {
                                    step_index: i,
                                    label: cmd.label(),
                                    stderr: "done blocked: tests_passed = false".into(),
                                    exit_code: 1,
                                    culprit_file: None,
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
                                if cmd.is_run_tests() {
                                    self.ctx.tests_passed = true;
                                }
                                // v5.8.1: run: cargo test أيضاً يُعتبر نجاح اختبارات
                                if let crate::protocol::Cmd::Run { command } = cmd {
                                    let lc = command.to_lowercase();
                                    if (lc.contains("cargo test")
                                        || lc.contains("go test")
                                        || lc.contains("pytest")
                                        || lc.contains("npm test"))
                                        && (r.stdout.contains("passed")
                                            || r.stdout.contains("ok"))
                                    {
                                        self.ctx.tests_passed = true;
                                    }
                                }
                                self.ctx.successful_hashes.insert(cmd_hash.clone());
                            }
                            Ok(r) => {
                                let err: String = r.stderr.chars().take(3000).collect();
                                println!("   ✗ {}", err);
                                // تسجيل الفشل — تابع بقية الأوامر
                                if cmd.is_run_tests() {
                                    self.ctx.tests_passed = false;
                                }
                                self.ctx.failed_steps.push(FailedStep {
                                    step_index: i,
                                    label: cmd.label(),
                                    stderr: err.clone(),
                                    exit_code: r.exit_code,
                                    culprit_file: FailedStep::extract_culprit(&err),
                                });
                            }
                            Err(e) => {
                                println!("   ❌ {}", e);
                                self.ctx.failed_steps.push(FailedStep {
                                    step_index: i,
                                    label: cmd.label(),
                                    stderr: e.to_string(),
                                    exit_code: -1,
                                    culprit_file: None,
                                });
                            }
                        }
                    }

                    // بعد كل الأوامر — قرر الحالة التالية
                    if matches!(self.state, AgentState::Executing) {
                        if self.ctx.tests_passed {
                            // ─── Mutation Check v1.3 ───
                            if let Some(fail) = self.run_mutation_check().await {
                                self.ctx.failed_steps.push(fail);
                                snapshot.rollback(); // v7.4: rollback on mutation failure
                                self.state = AgentState::Repairing;
                            } else {
                                snapshot.commit(); // v7.4: accept changes on success
                                self.ctx.save_hashes(&self.executor.workspace);
                                println!("\n✅ Goal complete! Tests passed.");
                                println!("SEL_SUCCESS");
                                self.state = AgentState::Done;
                            }
                        } else {
                            // v7.4.1: Deferred Rollback — DO NOT rollback here.
                            // Accept current changes so Repair phase can patch them.
                            snapshot.commit(); 
                            self.state = AgentState::Repairing;
                        }
                    } else {
                        // State changed during execution (e.g., to Done or Repairing)
                        if matches!(self.state, AgentState::Done) {
                            snapshot.commit();
                        } else {
                            snapshot.rollback();
                        }
                    }
                }

                // ─── Repairing ────────────────────────────────
                // استدعاء LLM واحد لخطة إصلاح
                AgentState::Repairing => {
                    // ─── InfraError: retry بدون LLM ───────────────
                    {
                        let all_err = self
                            .ctx
                            .failed_steps
                            .iter()
                            .map(|f| f.stderr.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        if FailureKind::classify(&all_err) == FailureKind::InfraError {
                            let infra_retries = self.ctx.repair_attempts;
                            if infra_retries >= 3 {
                                self.state = AgentState::Failed(
                                    "Infrastructure failure: network/API unavailable after 3 retries.".into()
                                );
                                continue;
                            }
                            let wait = [15u64, 45, 120][infra_retries as usize];
                            println!(
                                "\n⚠️  Infra error — retry {}/3 in {}s (no LLM call)...",
                                infra_retries + 1,
                                wait
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                            self.ctx.repair_attempts += 1;
                            self.ctx.failed_steps.clear();
                            self.state = AgentState::Planning;
                            continue;
                        }
                    }
                    self.ctx.repair_attempts += 1;
                    self.send_event(
                        "repair",
                        None,
                        Some(&format!("Repair attempt {}", self.ctx.repair_attempts)),
                        None,
                        None,
                    );

                    let repair_limit = self
                        .ctx
                        .current_failure_kind
                        .as_ref()
                        .map(|k: &FailureKind| k.max_attempts())
                        .unwrap_or(self.ctx.max_repairs);
                    if self.ctx.repair_attempts > repair_limit {
                        let reason = format!(
                            "Failed after {} repair attempts. Last errors:\n{}",
                            self.ctx.repair_attempts - 1,
                            self.ctx
                                .failed_steps
                                .iter()
                                .map(|f| format!("  • {}: {}", f.label, {
                                    let s = &f.stderr;
                                    let start = s.len().saturating_sub(1000);
                                    &s[start..]
                                }))
                                .collect::<Vec<_>>()
                                .join("\n")
                        );
                        self.state = AgentState::Failed(reason);
                        continue;
                    }

                    // v7.3: quick_fix بدون LLM (ModuleNotFoundError, GoUndefined)
                    {
                        let qf_stderr = self
                            .ctx
                            .failed_steps
                            .iter()
                            .map(|f| f.stderr.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        if let Some(fix) = crate::memory::quick_fix(&qf_stderr) {
                            match fix {
                                crate::memory::QuickFix::InstallPackage { command } => {
                                    println!("   ⚡ QuickFix: {}", command);
                                    let parts: Vec<&str> = command.split_whitespace().collect();
                                    if parts.len() >= 2 {
                                        let venv_pip = self.executor.workspace.join("venv/bin/pip3");
                                        let pip = if venv_pip.exists() {
                                            venv_pip.to_string_lossy().to_string()
                                        } else {
                                            "pip3".to_string()
                                        };
                                        let pkgs = &parts[2..];
                                        let _ = std::process::Command::new(&pip)
                                            .arg("install")
                                            .args(pkgs)
                                            .current_dir(&self.executor.workspace)
                                            .output();
                                        println!("   ✅ QuickFix installed: {}", pkgs.join(" "));
                                        self.ctx.failed_steps.clear();
                                        self.state = AgentState::Executing;
                                        continue;
                                    }
                                }
                                crate::memory::QuickFix::AddGoImport { symbol } => {
                                    println!("   ⚡ QuickFix Go import: {}", symbol);
                                    // executor autofix_go_undefined_import يعالجها
                                    // هنا نمرر للـ repair مع hint
                                }
                            }
                        }
                    }
                    let early_stderr = self
                        .ctx
                        .failed_steps
                        .iter()
                        .map(|f| f.stderr.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let early_kind = FailureKind::classify(&early_stderr);
                    let display_limit = early_kind.max_attempts();
                    println!(
                        "\n🔧 Repair {}/{}...",
                        self.ctx.repair_attempts, display_limit
                    );

                    let ws = &self.executor.workspace;

                    // ─── all_stderr أولاً (يحتاجه Context Budget) ───
                    let all_stderr = self
                        .ctx
                        .failed_steps
                        .iter()
                        .map(|f| f.stderr.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");

                    // Dynamic file discovery — يقرأ كل الملفات التي كُتبت في الـ plan
                    let written_files: Vec<String> = self
                        .plan
                        .iter()
                        .filter_map(|c| match c {
                            crate::protocol::Cmd::WriteFile { path, .. } => Some(path.clone()),
                            _ => None,
                        })
                        .collect();
                    // ─── Context Budget v1.3 ───
                    let all_workspace_files: Vec<std::path::PathBuf> = written_files
                        .iter()
                        .map(|f| ws.join(f))
                        .filter(|p| p.exists())
                        .collect();
                    let repair_ctx = crate::context::RepairContext {
                        stderr: all_stderr.clone(),
                        recent_edits: self
                            .ctx
                            .failed_steps
                            .iter()
                            .filter_map(|s| {
                                let p = ws.join(&s.label);
                                if p.exists() {
                                    Some(p)
                                } else {
                                    None
                                }
                            })
                            .collect(),
                        max_tokens: crate::context::MAX_REPAIR_TOKENS,
                        force_include: if all_stderr.trim().is_empty() {
                            all_workspace_files.clone()
                        } else {
                            vec![]
                        },
                        // Multi-file Repair Memory — الملفات المسبّبة مباشرة
                        culprit_files: self
                            .ctx
                            .failed_steps
                            .iter()
                            .filter_map(|s| s.culprit_file.clone())
                            .collect(),
                        context_config: Some(self.context_config.clone()),
                    };
                    if std::env::var("SEL_DEBUG").is_ok() {
                        let culprits: Vec<_> = self
                            .ctx
                            .failed_steps
                            .iter()
                            .filter_map(|s| s.culprit_file.as_ref())
                            .collect();
                        if !culprits.is_empty() {
                            println!("  🎯 Culprit files: {:?}", culprits);
                        }
                    }
                    let (selected_files, budget_report) =
                        crate::context::select_repair_files(&all_workspace_files, &repair_ctx);
                    if std::env::var("SEL_DEBUG").is_ok() {
                        budget_report.print();
                    }
                    // Token-Aware Repair v1.2
                    // بعض الأخطاء لا تحتاج محتوى الملفات — فقط أسماءها
                    let failure_kind = FailureKind::classify(&all_stderr);
                    self.ctx.current_failure_kind = Some(failure_kind.clone());
                    let repair_hint = failure_kind.repair_hint();
                    println!("   🔍 Failure type: {:?}", failure_kind);

                    let dep_only = matches!(
                        failure_kind,
                        FailureKind::ImportError | FailureKind::NodeTestError
                    );


                    // v6.0: Always inject full file content for ALL repairs (not just PatchError)
                    // This ensures the LLM always sees the current state of files before patching
                    let patch_error_context: String = {
                        let mut patch_ctx = String::new();
                        let is_patch_error = all_stderr.contains("search block not found")
                            || all_stderr.contains("search block found");
                        // Collect all failed files from current repair cycle
                        let mut files_to_inject: Vec<String> = Vec::new();
                        // From patch errors - extract file names
                        for line in all_stderr.lines() {
                            if line.contains("search block not found in '") {
                                if let Some(start) = line.find("in '") {
                                    let rest = &line[start + 4..];
                                    if let Some(end) = rest.find('\'') {
                                        files_to_inject.push(rest[..end].to_string());
                                    }
                                }
                            }
                        }
                        // From failed steps - extract culprit files
                        for step in &self.ctx.failed_steps {
                            if let Some(ref cf) = step.culprit_file {
                                let name = std::path::Path::new(cf)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(cf.as_str())
                                    .to_string();
                                if !files_to_inject.contains(&name) {
                                    files_to_inject.push(name);
                                }
                            }
                        }
                        // Always inject files on AssertionError / PatchError (2nd attempt+)
                        let should_inject_all = is_patch_error
                            || self.ctx.repair_attempts >= 2
                            || matches!(self.ctx.current_failure_kind,
                                Some(FailureKind::AssertionError) | Some(FailureKind::Unknown));

                        for file_name in &files_to_inject {
                            let full_path = self.executor.workspace.join(file_name);
                            if let Ok(content) = std::fs::read_to_string(&full_path) {
                                patch_ctx.push_str(&format!(
                                    "\n\n⚠️ CURRENT FILE CONTENT of '{}' (use this EXACT text for search blocks):\n```\n{}\n```",
                                    file_name, content
                                ));
                                println!("   📖 v6.0: injecting full content of '{}' for repair", file_name);
                            }
                        }
                        // On 3rd attempt+ with AssertionError, also inject ALL workspace source files
                        if should_inject_all && files_to_inject.is_empty() {
                            for ws_file in &all_workspace_files {
                                let name = ws_file.file_name()
                                    .and_then(|n: &std::ffi::OsStr| n.to_str())
                                    .unwrap_or_default()
                                    .to_string();
                                // Only source files, skip tests
                                if !name.contains("test") && !name.contains("spec") {
                                    if let Ok(content) = std::fs::read_to_string(ws_file) {
                                        patch_ctx.push_str(&format!(
                                            "\n\n📄 SOURCE FILE '{}' (copy exact text for patches):\n```\n{}\n```",
                                            name, content
                                        ));
                                        println!("   📖 v6.0: injecting source file '{}'", name);
                                    }
                                }
                            }
                        }
                        patch_ctx
                    };

                    let files_context: String = if dep_only {
                        // أرسل أسماء الملفات فقط — توفير tokens
                        let names: Vec<String> = selected_files
                            .iter()
                            .map(|sf| {
                                sf.path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("?")
                                    .to_string()
                            })
                            .collect();
                        if std::env::var("SEL_DEBUG").is_ok() {
                            println!(
                                "  ⚡ Token-Aware: sending file names only ({} files)",
                                names.len()
                            );
                        }
                        format!("FILES IN PROJECT: {}", names.join(", "))
                    } else {
                        // v5.4: Smart Repair Context
                        // استخرج مواقع الأخطاء من stderr
                        let error_locs = crate::chunker::extract_error_locations(&all_stderr);
                        if !error_locs.is_empty() {
                            println!(
                                "   🎯 v5.4: error locations found: {} — using chunks only",
                                error_locs.len()
                            );
                        }
                        selected_files
                            .iter()
                            .map(|sf| {
                                let f = sf.path.to_string_lossy();
                                let lang = if f.ends_with(".py") {
                                    "python"
                                } else if f.ends_with(".rs") {
                                    "rust"
                                } else if f.ends_with(".js") {
                                    "javascript"
                                } else if f.ends_with(".go") {
                                    "go"
                                } else if f.ends_with(".toml") {
                                    "toml"
                                } else {
                                    "text"
                                };
                                // v5.4: Smart Repair Context — chunk حول الخطأ فقط
                                let smart =
                                    crate::chunker::get_file_content_smart(&sf.path, &error_locs);
                                let file_content = match smart {
                                    Ok(ref s) => {
                                        if s.is_chunk() {
                                            println!(
                                                "   ✂️  v5.4: {} → chunk only",
                                                sf.path
                                                    .file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy()
                                            );
                                        }
                                        s.content_for_prompt(&f)
                                    }
                                    Err(_) => sf.content.clone(),
                                };
                                format!("{}:\n```{}\n{}\n```", f, lang, file_content)
                            })
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    };


                    // تصنيف نوع الفشل — يجب أن يكون قبل files_context
                    let errors = self
                        .ctx
                        .failed_steps
                        .iter()
                        .map(|f| {
                            format!("Step '{}' failed (exit {}):\n{}", f.label, f.exit_code, {
                                let s = &f.stderr;
                                let start = s.len().saturating_sub(2000);
                                &s[start..]
                            })
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    let network_note = if errors.contains("Network is unreachable")
                        || errors.contains("Timeout after")
                    {
                        "\n\nNETWORK UNAVAILABLE: Use ONLY Python stdlib. NO pandas, NO requests."
                    } else {
                        ""
                    };

                    // Mutation Enforcement v1.3
                    let mutation_note = if errors.contains("WEAK TESTS") {
                        "\n\nMUTATION ENFORCEMENT: Your tests are too weak — they passed on broken code.\n                         YOU MUST strengthen the test file:\n                         1. Add assert statements with EXACT expected values (e.g. assert result == 42).\n                         2. Test edge cases: negative numbers, zero, empty input.\n                         3. Each function must have at least 2 independent assertions.\n                         DO NOT modify the source file — only improve the test file."
                    } else {
                        ""
                    };
                    // ─── Structured Repair Memory v1.3 ───
                    let attempt_note = if self.ctx.repair_attempts > 1 {
                        match &self.previous_error {
                            Some(prev) => format!(
                                "ATTEMPT {}/{}: Previous fix failed.\n  Previous error: {}\n  New error:      {}\n  Your fix changed the problem but did not solve it. Try a different approach.",
                                self.ctx.repair_attempts,
                                display_limit,
                                prev.chars().take(300).collect::<String>(),
                                all_stderr.chars().take(300).collect::<String>()
                            ),
                            None => format!(
                                "ATTEMPT {}/{}: Previous fix failed — try a completely different approach.",
                                self.ctx.repair_attempts, display_limit
                            ),
                        }
                    } else {
                        format!(
                            "ATTEMPT {}/{}: First repair attempt.",
                            self.ctx.repair_attempts, display_limit
                        )
                    };
                    let loop_warning = if self.repair_fingerprints.len() > 1
                        && self.repair_fingerprints.last()
                            == self
                                .repair_fingerprints
                                .get(self.repair_fingerprints.len().saturating_sub(2))
                    {
                        "\n\nWARNING: You are repeating the same fix. This approach failed before. Try something completely different."
                    } else {
                        ""
                    };
                    // احفظ الخطأ الحالي للمحاولة القادمة
                    self.previous_error = Some(all_stderr.chars().take(500).collect());

                    // Repair History Guard v1.2 — تجنب تكرار نفس الإصلاح
                    let fingerprint: u64 = all_stderr
                        .bytes()
                        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
                    if self.repair_fingerprints.contains(&fingerprint) {
                        println!("   ⚠️  Repair loop detected — same error repeated. Forcing different strategy.");
                    } else {
                        self.repair_fingerprints.push(fingerprint);
                    }
                    // v5.8: Failure Memory hints
                    let memory_hint = self.failure_memory.get_hints(
                        &format!("{:?}", failure_kind),
                        &all_stderr.chars().take(80).collect::<String>(),
                    );

                    let patch_note = if !files_context.starts_with("FILES IN PROJECT:") {
                        "\n\n⚠ REPAIR RULES — MANDATORY:\n1. DO NOT use write_file on files that already exist — this resets them to broken state.\n2. Use patch_file to fix existing files. Copy search text EXACTLY from CURRENT FILES above.\n3. write_file is FORBIDDEN for existing files during repair.\nWRONG: {\"type\":\"write_file\",\"path\":\"calc.py\",...}  ← overwrites with wrong code\nRIGHT: {\"type\":\"patch_file\",\"path\":\"calc.py\",\"search\":\"return a - b\",\"replace\":\"return a + b\"}"
                    } else {
                        ""
                    };

                    // v5.1: Reference File Support
                    let ref_file_context = if let Some(ref ref_path) = self.context_config.ref_file
                    {
                        crate::context::read_ref_file(ref_path)
                            .map(|content| {
                                format!(
                                    "\n\nREFERENCE FILE ({}):\n```\n{}\n```",
                                    ref_path.display(),
                                    content
                                )
                            })
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };

                    let prompt = crate::constitution::CONSTITUTION.to_string() + &format!(
                        "Goal: {}{}{}{}{}{}\n\nHINT: {}\n\n{}\n\nFAILED STEPS:\n{}\n\nCURRENT FILES:\n{}{}\n\
                         Fix ALL issues. Provide complete corrected plan.",
                        self.goal, network_note, mutation_note, patch_note, ref_file_context, memory_hint, repair_hint, &(attempt_note.to_string() + loop_warning), errors, files_context, patch_error_context
                    );

                    // Protocol Resilience v1.3
                    match self.plan_with_resilience(prompt).await {
                        Ok(commands) => {
                            // v7.4: Constraint Engine — filter repair commands too
                            let commands_count = commands.len();
                            let env = crate::constraint_engine::ProjectEnv::detect(&self.executor.workspace);
                            let ce_state = crate::constraint_engine::ProjectState::scan(&self.executor.workspace);
                            let commands = match crate::constraint_engine::apply(commands, &env, &ce_state) {
                                crate::constraint_engine::ConstraintResult::Ok(filtered) => {
                                    if filtered.len() < commands_count {
                                        println!("   🛡️ Constraint Engine (repair): filtered {} commands",
                                            commands_count - filtered.len());
                                    }
                                    filtered
                                }
                                crate::constraint_engine::ConstraintResult::Fatal(reason) => {
                                    println!("   ⛔ Constraint Engine (repair): {}", reason);
                                    self.state = AgentState::Failed(reason);
                                    continue;
                                }
                            };
                            println!("   ✓ Repair plan: {} commands", commands.len());
                            self.plan = commands;
                            self.state = AgentState::Executing;
                        }
                        Err(e) => {
                            println!("   ⚠ Repair plan parse failed: {}", e);
                            self.ctx.failed_steps.push(crate::types::FailedStep {
                                step_index: 0,
                                label: "repair_planning".into(),
                                stderr: format!("Protocol parse error: {}", e),
                                exit_code: -2,
                                culprit_file: None,
                            });
                        }
                    }
                }

                // ─── Terminal States ───────────────────────────
                AgentState::Done => {
                    let repairs = self.ctx.repair_attempts.saturating_sub(1);
                    // v5.8: حفظ الـ memory إذا كان هناك repair ناجح
                    if self.ctx.repair_attempts > 0 {
                        // v5.8: نستخدم last_failed_steps لأن failed_steps تُمسح في reset_for_repair
                        let repair_steps = if !self.ctx.last_failed_steps.is_empty() {
                            &self.ctx.last_failed_steps
                        } else {
                            &self.ctx.failed_steps
                        };
                        let failure_kind = crate::types::FailureKind::classify(
                            &repair_steps
                                .iter()
                                .map(|f| f.stderr.as_str())
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                        let error_sig = repair_steps
                            .first()
                            .map(|f| f.stderr.chars().take(120).collect::<String>())
                            .unwrap_or_default();
                        let fix_summary = self
                            .plan
                            .iter()
                            .filter_map(|c| match c {
                                crate::protocol::Cmd::PatchFile { path, .. } => {
                                    Some(format!("patch_file {}", path))
                                }
                                crate::protocol::Cmd::WriteFile { path, .. } => {
                                    Some(format!("write_file {}", path))
                                }
                                crate::protocol::Cmd::Run { command } => {
                                    let short: String = command.chars().take(40).collect();
                                    Some(format!("run {}", short))
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        if !error_sig.is_empty() && !fix_summary.is_empty() {
                            self.failure_memory.record_success(
                                &format!("{:?}", failure_kind),
                                &error_sig,
                                &fix_summary,
                            );
                            println!("   💾 v5.8: memory saved ({:?})", failure_kind);
                        }
                    }
                    let elapsed = self
                        .ctx
                        .start_time
                        .map(|s: std::time::Instant| s.elapsed().as_secs())
                        .unwrap_or(0);
                    let ms = if self.ctx.mutations_total > 0 {
                        self.ctx.mutations_killed as f64 / self.ctx.mutations_total as f64
                    } else {
                        -1.0
                    };
                    let _ = report_run(&self.goal, true, repairs as i64, elapsed, ms).await;
                    return Ok(());
                }
                AgentState::Failed(reason) => {
                    println!("\n❌ Agent failed: {}", reason);
                    println!("SEL_FAILED: {}", reason.lines().next().unwrap_or("unknown"));

                    // v7.4.1: Final Rollback on failure
                    if let Some(mut snap) = self.initial_snapshot.take() {
                        snap.rollback();
                    }

                    let repairs = self.ctx.repair_attempts as i64;
                    let elapsed = self
                        .ctx
                        .start_time
                        .map(|s: std::time::Instant| s.elapsed().as_secs())
                        .unwrap_or(0);
                    let ms = if self.ctx.mutations_total > 0 {
                        self.ctx.mutations_killed as f64 / self.ctx.mutations_total as f64
                    } else {
                        -1.0
                    };
                    let _ = report_run(&self.goal, false, repairs, elapsed, ms).await;
                    return Err(anyhow::anyhow!("SEL_FAILED"));
                }
            }
        }
    }
}

async fn report_run(
    goal: &str,
    success: bool,
    repairs: i64,
    duration_secs: u64,
    mutation_score: f64,
) -> Result<()> {
    let model =
        std::env::var("SEL_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string());
    let body = serde_json::json!({
        "goal": &goal[..goal.len().min(200)],
        "success": success,
        "repairs": repairs,
        "duration_secs": duration_secs,
        "mutation_score": mutation_score,
        "model": model
    });
    let client = reqwest::Client::new();
    let _ = client
        .post("http://localhost:8777/api/runs")
        .json(&body)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;
    Ok(())
}
