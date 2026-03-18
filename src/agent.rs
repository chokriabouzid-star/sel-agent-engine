// src/agent.rs — v1.3: State Machine

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
    previous_error:      Option<String>,
    repair_fingerprints: Vec<u64>,   // Repair History Guard v1.2
}

impl Agent {
    pub fn new(api_key: String, workspace: PathBuf, goal: String, max_repairs: u8) -> Self {
        Self {
            state:    AgentState::Planning,
            ctx:      ExecutionContext::new(max_repairs),
            executor: SafeExecutor::new(workspace, 120),
            llm:      LlmClient::new(api_key),
            goal,
            plan:               Vec::new(),
            previous_error:     None,
            repair_fingerprints: Vec::new(),
        }
    }
    pub fn repair_count(&self) -> usize {
        self.ctx.repair_attempts as usize
    }
    fn send_event(&self, event_type: &str, step: Option<&str>, detail: Option<&str>, success: Option<bool>, mutation: Option<f64>) {
        let model = std::env::var("SEL_MODEL")
            .unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string());
        let body = serde_json::json!({
            "event_type": event_type,
            "goal": &self.goal,
            "step": step.unwrap_or(""),
            "detail": detail.unwrap_or(""),
            "success": success,
            "mutation": mutation,
            "repairs": self.ctx.repair_attempts,
            "model": &self.llm.model,
            "timestamp": ""
        });
        let url = std::env::var("SEL_OBSERVATORY")
            .unwrap_or_else(|_| "http://localhost:8777".to_string());
        let _ = std::process::Command::new("curl")
            .args(["-s", "-X", "POST",
                   &format!("{}/api/event", url),
                   "-H", "Content-Type: application/json",
                   "-d", &body.to_string()])
            .output();
    }

    pub fn mutation_score(&self) -> f64 {
        if self.ctx.mutations_total > 0 {
            self.ctx.mutations_killed as f64 / self.ctx.mutations_total as f64
        } else { -1.0 }
    }

    // ══════════════════════════════════════════════════════════
    // Goal Validator v1.2
    fn validate_goal(goal: &str) -> Option<String> {
        let g = goal.to_lowercase();
        let len = goal.trim().len();
        if len < 10 {
            return Some("Goal too short.".to_string());
        }
        let real_keywords = ["fix", "implement", "refactor",
            "update", "migrate", "failing", "crate", "existing", "workspace"];
        if real_keywords.iter().any(|kw| g.contains(kw)) {
            return None;
        }
        let has_test = g.contains("test") || g.contains("pytest")
            || g.contains("assert") || g.contains("spec") || g.contains("verify");
        if !has_test {
            return Some("Goal has no test requirement — add tests to verify.".to_string());
        }
        let vague = (g.contains("test") || g.contains("assert"))
            && (g.contains("some value") || g.contains("correct value"));
        if vague {
            return Some("Ambiguous values — specify exact expected values.".to_string());
        }
        None
    }

    // ══════════════════════════════════════════════════════════


    // ══════════════════════════════════════════════════════════
    // Protocol Resilience v1.3
    async fn plan_with_resilience(&self, base_prompt: String) -> Result<Vec<Cmd>, String> {
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
                Ok(response) => match crate::protocol::parse(&response) {
                    Ok(plan) => {
                        if attempt > 0 {
                            println!("   ✅ Protocol retry {} succeeded.", attempt);
                        }
                        return Ok(plan.commands);
                    }
                    Err(e) => {
                        if attempt < MAX_RETRIES {
                            println!(
                                "   ⚠ JSON parse failed (attempt {}/{}) — retrying with simplified prompt...",
                                attempt + 1, MAX_RETRIES + 1
                            );
                            println!("     Reason: {}", e.to_string().lines().next().unwrap_or("?"));
                        } else {
                            return Err(format!(
                                "JSON parse failed after {} attempts: {}",
                                MAX_RETRIES + 1, e
                            ));
                        }
                    }
                },
                Err(e) => return Err(e.to_string()),
            }
        }
        unreachable!()
    }
    // ══════════════════════════════════════════════════════════

    pub async fn run(&mut self) -> Result<()> {
        self.ctx.start_time = Some(std::time::Instant::now());
        self.send_event("start", None, None, None, None);
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
                    // Goal Validator v1.2 — يرفض الـ goal الغامض صامتاً
                    if let Some(reason) = Self::validate_goal(&self.goal) {
                        println!("\n❌ Invalid goal: {}", reason);
                        println!("SEL_FAILED: {}", reason);
                        self.state = AgentState::Done;
                        break Ok(());
                    }
                    println!("\n🧠 Planning...");
                    self.send_event("step", Some("Planning"), Some("Generating execution plan"), None, None);
                    // كشف لغة المشروع من الملفات الموجودة في workspace
                    let ws = &self.executor.workspace;
                    let has_cargo = ws.join("Cargo.toml").exists();
                    let has_package_json = ws.join("package.json").exists();
                    let has_go_mod = ws.join("go.mod").exists();
                    let lang_hint = if has_cargo {
                        "\nCRITICAL: This is a RUST project (Cargo.toml exists). Write ONLY Rust code. Do NOT create Python or JS files."
                    } else if has_package_json {
                        "\nCRITICAL: This is a Node.js project (package.json exists). Write ONLY JS/TS code."
                    } else if has_go_mod {
                        "\nCRITICAL: This is a Go project (go.mod exists). Write ONLY Go code."
                    } else { "" };
                    // قراءة الملفات الموجودة بشكل recursive وإضافتها للـ prompt
                    let existing_files = {
                        let mut files_ctx = String::new();
                        let extensions = [".rs", ".py", ".js", ".ts", ".go"];
                        // walk recursive حتى عمق 3
                        fn walk(dir: &std::path::Path, ws: &std::path::Path,
                                exts: &[&str], out: &mut String, depth: u8, count: &mut usize) {
                            if depth > 3 { return; }
                            let Ok(entries) = std::fs::read_dir(dir) else { return };
                            for entry in entries.flatten() {
                                let p = entry.path();
                                if p.is_dir() {
                                    let name = p.file_name()
                                        .and_then(|n| n.to_str()).unwrap_or("");
                                    if !matches!(name, "target"|".git"|"node_modules"|"venv") {
                                        walk(&p, ws, exts, out, depth + 1, count);
                                    }
                                } else {
                                    let name = p.file_name()
                                        .and_then(|n| n.to_str()).unwrap_or("");
                                    let is_code = exts.iter().any(|e| name.ends_with(e));
                                    if is_code {
                                        if let Ok(content) = std::fs::read_to_string(&p) {
                                            if content.len() > 50 {
                                                let rel = p.strip_prefix(ws).unwrap_or(&p);
                                                out.push_str(&format!(
                                                    "\n\nEXISTING FILE: {}\n```\n{}\n```",
                                                    rel.display(),
                                                    &content[..content.len().min(2500)]
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let mut file_count = 0usize;
                        walk(ws, ws, &extensions, &mut files_ctx, 0, &mut file_count);
                        if !files_ctx.is_empty() {
                            format!("\n\nCRITICAL — EXISTING FILES (you MUST preserve ALL existing code and APPEND only):{}", files_ctx)
                        } else {
                            String::new()
                        }
                    };
                    let prompt = format!(
                        "Goal: {}{}{}\n\nProvide the complete execution plan.",
                        self.goal, lang_hint, existing_files
                    );
                    // Protocol Resilience v1.3
                    match self.plan_with_resilience(prompt).await {
                        Ok(commands) => {
                            println!("   ✓ {} commands\n", commands.len());
                            self.plan  = commands;
                            self.state = AgentState::Executing;
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
                    let plan = self.plan.clone();
                    let total = plan.len();

                    for (i, cmd) in plan.iter().enumerate() {
                        println!("[{}/{}] {}", i + 1, total, cmd.label());
                        { let _lbl = cmd.label(); let _prog = format!("{}/{}", i+1, total); self.send_event("step", Some(&_lbl), Some(&_prog), None, None); }

                        // skip الخطوات الناجحة سابقاً
                        let cmd_hash = cmd.hash();
                        // pip install يُعاد تشغيله إذا كان venv غير موجود (مثلاً بعد rm -rf venv)
                        let is_pip = cmd.label().contains("pip");
                        let venv_ok = self.executor.workspace.join("venv/bin/pip3").exists()
                                   || self.executor.workspace.join("venv/bin/pip").exists();
                        let skip_allowed = !cmd.is_run_tests() && !cmd.is_write_file()
                                        && !cmd.is_patch_file()
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
                                let msg = if let Cmd::Done { message } = cmd { message } else { "Goal complete" };
                                // ─── Mutation Check v1.3 ───
                                let impl_source = self.plan.iter().find_map(|c| match c {
                                    crate::protocol::Cmd::WriteFile { path, .. }
                                        if (path.ends_with(".py") || path.ends_with(".go")
                                            || path.ends_with(".js") || path.ends_with(".ts")
                                            || path.ends_with(".rs"))
                                           && !path.contains("test")
                                           && !path.contains("Cargo.toml")
                                           && !path.contains("go.mod")
                                           && !path.contains("package.json")
                                           && !path.contains("jest.config")
                                           && !path.contains("tsconfig") => Some(path.clone()),
                                    _ => None,
                                });
                                let mut mutation_passed = true;
                                if let Some(src) = impl_source {
                                    use crate::executor::MutationResult;
                                    println!("\n🧬 Mutation check on {}...", src);
                                    match self.executor.mutation_check(&src).await {
                                        MutationResult::Weak(orig_line, mutd_line) => { self.ctx.mutations_total += 1;
                                            println!("   ⚠️  Tests are WEAK — triggering repair (Mutation Enforcement v1.3).");
                                            println!("     Survived mutation: [{}] → [{}]", orig_line, mutd_line);
                                            mutation_passed = false;
                                            self.ctx.failed_steps.push(crate::types::FailedStep {
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
                                                culprit_file: Some(src.clone()),
                                            });
                                        }
                                        MutationResult::Strong  => { println!("   ✅ Tests are solid.");
                                        self.send_event("mutation", None, None, None, Some(1.0)); self.ctx.mutations_total += 1; self.ctx.mutations_killed += 1; }
                                        MutationResult::Skipped => println!("   ⏭  Mutation check skipped."),
                                    }
                                }
                                // ─────────────────────────────
                                if mutation_passed {
                                    self.ctx.save_hashes(&self.executor.workspace);
                                    println!("\n✅ {}", if msg.is_empty() { "Goal complete!" } else { msg });
                                    println!("SEL_SUCCESS");
                                    self.send_event("done", None, None, Some(true), Some(self.mutation_score()));
                                    self.state = AgentState::Done;
                                } else {
                                    self.state = AgentState::Repairing;
                                }
                            } else {
                                println!("   ⛔ done rejected — tests must pass first");
                                self.ctx.failed_steps.push(FailedStep {
                                    step_index:   i,
                                    label:        cmd.label(),
                                    stderr:       "done blocked: tests_passed = false".into(),
                                    exit_code:    1,
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
                                if cmd.is_run_tests() { self.ctx.tests_passed = true; }
                                self.ctx.successful_hashes.insert(cmd_hash.clone());
                            }
                            Ok(r) => {
                                let err: String = r.stderr.chars().take(3000).collect();
                                println!("   ✗ {}", err);
                                // تسجيل الفشل — تابع بقية الأوامر
                                if cmd.is_run_tests() { self.ctx.tests_passed = false; }
                                self.ctx.failed_steps.push(FailedStep {
                                    step_index:   i,
                                    label:        cmd.label(),
                                    stderr:       err.clone(),
                                    exit_code:    r.exit_code,
                                    culprit_file: FailedStep::extract_culprit(&err),
                                });
                            }
                            Err(e) => {
                                println!("   ❌ {}", e);
                                self.ctx.failed_steps.push(FailedStep {
                                    step_index:   i,
                                    label:        cmd.label(),
                                    stderr:       e.to_string(),
                                    exit_code:    -1,
                                    culprit_file: None,
                                });
                            }
                        }
                    }

                    // بعد كل الأوامر — قرر الحالة التالية
                    if matches!(self.state, AgentState::Executing) {
                        if self.ctx.tests_passed {
                            // ─── Mutation Check v1.3 ───
                            let py_source = self.plan.iter().find_map(|c| match c {
                                crate::protocol::Cmd::WriteFile { path, .. }
                                    if path.ends_with(".py") && !path.contains("test") => Some(path.clone()),
                                _ => None,
                            });
                            if let Some(src) = py_source {
                                use crate::executor::MutationResult;
                                println!("\n🧬 Mutation check on {}...", src);
                                match self.executor.mutation_check(&src).await {
                                    MutationResult::Weak(orig_line, mutd_line) => {
                                        println!("   ⚠️  Tests are WEAK — triggering repair (Mutation Enforcement v1.3).");
                                        println!("     Survived mutation: [{}] → [{}]", orig_line, mutd_line);
                                        self.ctx.failed_steps.push(crate::types::FailedStep {
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
                                            culprit_file: Some(src.clone()),
                                        });
                                        self.state = AgentState::Repairing;
                                    }
                                    MutationResult::Strong  => {
                                        println!(" ✅ Tests are solid.");
                                        self.ctx.save_hashes(&self.executor.workspace);
                                        println!("\n✅ Goal complete! Tests passed.");
                                        println!("SEL_SUCCESS");
                                        self.state = AgentState::Done;
                                    }
                                    MutationResult::Skipped => {
                                        println!(" ⏭  Skipped.");
                                        self.ctx.save_hashes(&self.executor.workspace);
                                        println!("\n✅ Goal complete! Tests passed.");
                                        println!("SEL_SUCCESS");
                                        self.state = AgentState::Done;
                                    }
                                }
                            } else {
                                self.ctx.save_hashes(&self.executor.workspace);
                                println!("\n✅ Goal complete! Tests passed.");
                                println!("SEL_SUCCESS");
                                self.state = AgentState::Done;
                            }
                        } else {
                            self.state = AgentState::Repairing;
                        }
                    }
                }

                // ─── Repairing ────────────────────────────────
                // استدعاء LLM واحد لخطة إصلاح
                AgentState::Repairing => {
                    self.ctx.repair_attempts += 1;
                            self.send_event("repair", None, Some(&format!("Repair attempt {}", self.ctx.repair_attempts)), None, None);

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

                    // ─── all_stderr أولاً (يحتاجه Context Budget) ───
                    let all_stderr = self.ctx.failed_steps.iter()
                        .map(|f| f.stderr.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");

                    // Dynamic file discovery — يقرأ كل الملفات التي كُتبت في الـ plan
                    let written_files: Vec<String> = self.plan.iter()
                        .filter_map(|c| match c {
                            crate::protocol::Cmd::WriteFile { path, .. } => Some(path.clone()),
                            _ => None,
                        })
                        .collect();
                    // ─── Context Budget v1.3 ───
                    let all_workspace_files: Vec<std::path::PathBuf> = written_files.iter()
                        .map(|f| ws.join(f))
                        .filter(|p| p.exists())
                        .collect();
                    let repair_ctx = crate::context::RepairContext {
                        stderr:       all_stderr.clone(),
                        recent_edits: self.ctx.failed_steps.iter()
                            .filter_map(|s| {
                                let p = ws.join(&s.label);
                                if p.exists() { Some(p) } else { None }
                            })
                            .collect(),
                        max_tokens:    crate::context::MAX_REPAIR_TOKENS,
                        force_include: if all_stderr.trim().is_empty() {
                            all_workspace_files.clone()
                        } else {
                            vec![]
                        },
                        // Multi-file Repair Memory — الملفات المسبّبة مباشرة
                        culprit_files: self.ctx.failed_steps.iter()
                            .filter_map(|s| s.culprit_file.clone())
                            .collect(),
                    };
                    if std::env::var("SEL_DEBUG").is_ok() {
                        let culprits: Vec<_> = self.ctx.failed_steps.iter()
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
                    let repair_hint = failure_kind.repair_hint();
                    println!("   🔍 Failure type: {:?}", failure_kind);

                    let dep_only = matches!(failure_kind,
                        FailureKind::ImportError | FailureKind::NodeTestError
                    );
                    let files_context: String = if dep_only {
                        // أرسل أسماء الملفات فقط — توفير tokens
                        let names: Vec<String> = selected_files.iter()
                            .map(|sf| sf.path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("?")
                                .to_string())
                            .collect();
                        if std::env::var("SEL_DEBUG").is_ok() {
                            println!("  ⚡ Token-Aware: sending file names only ({} files)", names.len());
                        }
                        format!("FILES IN PROJECT: {}", names.join(", "))
                    } else {
                        selected_files.iter()
                            .map(|sf| {
                                let f = sf.path.to_string_lossy();
                                let lang = if f.ends_with(".py")   { "python" }
                                           else if f.ends_with(".rs")   { "rust" }
                                           else if f.ends_with(".js")   { "javascript" }
                                           else if f.ends_with(".go")   { "go" }
                                           else if f.ends_with(".toml") { "toml" }
                                           else { "text" };
                                format!("{}:\n```{}\n{}\n```", f, lang, sf.content)
                            })
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    };
                    // backward compat
                    let main_py = String::new();
                    let test_py = String::new();
                    let _ = (main_py.as_str(), test_py.as_str());

                    // تصنيف نوع الفشل — يجب أن يكون قبل files_context
                    let errors = self.ctx.failed_steps.iter()
                        .map(|f| format!("Step '{}' failed (exit {}):\n{}", f.label, f.exit_code, { let s = &f.stderr; let start = s.len().saturating_sub(2000); &s[start..] }))
                        .collect::<Vec<_>>()
                        .join("\n");

                    let network_note = if errors.contains("Network is unreachable") || errors.contains("Timeout after") {
                        "\n\nNETWORK UNAVAILABLE: Use ONLY Python stdlib. NO pandas, NO requests."
                    } else { "" };

                    // Mutation Enforcement v1.3
                    let mutation_note = if errors.contains("WEAK TESTS") {
                        "\n\n🧬 MUTATION ENFORCEMENT: Your tests are too weak.\nYOU MUST strengthen the test file:\n1. Add assert statements with EXACT expected values.\n2. Test edge cases: negative numbers, zero, empty input.\n3. Each function must have at least 2 independent assertions.\nDO NOT modify the source file."
                    } else { "" };
                    // Mutation Enforcement v1.3
                    let mutation_note = if errors.contains("WEAK TESTS") {
                        "\n\nMUTATION ENFORCEMENT: Your tests are too weak — they passed on broken code.\n                         YOU MUST strengthen the test file:\n                         1. Add assert statements with EXACT expected values (e.g. assert result == 42).\n                         2. Test edge cases: negative numbers, zero, empty input.\n                         3. Each function must have at least 2 independent assertions.\n                         DO NOT modify the source file — only improve the test file."
                    } else { "" };
                    // ─── Structured Repair Memory v1.3 ───
                    let attempt_note = if self.ctx.repair_attempts > 1 {
                        match &self.previous_error {
                            Some(prev) => format!(
                                "ATTEMPT {}/{}: Previous fix failed.\n  Previous error: {}\n  New error:      {}\n  Your fix changed the problem but did not solve it. Try a different approach.",
                                self.ctx.repair_attempts,
                                self.ctx.max_repairs,
                                prev.chars().take(300).collect::<String>(),
                                all_stderr.chars().take(300).collect::<String>()
                            ),
                            None => format!(
                                "ATTEMPT {}/{}: Previous fix failed — try a completely different approach.",
                                self.ctx.repair_attempts, self.ctx.max_repairs
                            ),
                        }
                    } else {
                        format!("ATTEMPT {}/{}: First repair attempt.", self.ctx.repair_attempts, self.ctx.max_repairs)
                    };
                    let loop_warning = if self.repair_fingerprints.len() > 1
                        && self.repair_fingerprints.last()
                            == self.repair_fingerprints.get(self.repair_fingerprints.len().saturating_sub(2)) {
                        "\n\nWARNING: You are repeating the same fix. This approach failed before. Try something completely different."
                    } else { "" };
                    // احفظ الخطأ الحالي للمحاولة القادمة
                    self.previous_error = Some(all_stderr.chars().take(500).collect());

                    // Repair History Guard v1.2 — تجنب تكرار نفس الإصلاح
                    let fingerprint: u64 = all_stderr.bytes()
                        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
                    if self.repair_fingerprints.contains(&fingerprint) {
                        println!("   ⚠️  Repair loop detected — same error repeated. Forcing different strategy.");
                    } else {
                        self.repair_fingerprints.push(fingerprint);
                    }
                    let patch_note = if !files_context.starts_with("FILES IN PROJECT:") {
                        "\n\n⚠ REPAIR RULES — MANDATORY:\n1. DO NOT use write_file on files that already exist — this resets them to broken state.\n2. Use patch_file to fix existing files. Copy search text EXACTLY from CURRENT FILES above.\n3. write_file is FORBIDDEN for existing files during repair.\nWRONG: {\"type\":\"write_file\",\"path\":\"calc.py\",...}  ← overwrites with wrong code\nRIGHT: {\"type\":\"patch_file\",\"path\":\"calc.py\",\"search\":\"return a - b\",\"replace\":\"return a + b\"}"
                    } else { "" };
                    let prompt = format!(
                        "Goal: {}{}{}{}\n\nHINT: {}\n\n{}\n\nFAILED STEPS:\n{}\n\nCURRENT FILES:\n{}\n\
                         Fix ALL issues. Provide complete corrected plan.",
                        self.goal, network_note, mutation_note, patch_note, repair_hint, attempt_note, errors, files_context
                    );

                    // Protocol Resilience v1.3
                    match self.plan_with_resilience(prompt).await {
                        Ok(commands) => {
                            println!("   ✓ Repair plan: {} commands", commands.len());
                            self.plan  = commands;
                            self.state = AgentState::Executing;
                        }
                        Err(e) => {
                            println!("   ⚠ Repair plan parse failed: {}", e);
                            self.ctx.failed_steps.push(crate::types::FailedStep {
                                step_index:   0,
                                label:        "repair_planning".into(),
                                stderr:       format!("Protocol parse error: {}", e),
                                exit_code:    -2,
                                culprit_file: None,
                            });
                        }
                    }
                }

                // ─── Terminal States ───────────────────────────
                AgentState::Done => {
                    let repairs = self.ctx.repair_attempts.saturating_sub(1);
                    let elapsed = self.ctx.start_time.map(|s: std::time::Instant| s.elapsed().as_secs()).unwrap_or(0);
                    let ms = if self.ctx.mutations_total > 0 { self.ctx.mutations_killed as f64 / self.ctx.mutations_total as f64 } else { -1.0 };
                    let _ = report_run(&self.goal, true, repairs as i64, elapsed, ms).await;
                    return Ok(());
                }
                AgentState::Failed(reason) => {
                    println!("\n❌ Agent failed: {}", reason);
                    println!("SEL_FAILED: {}", reason.lines().next().unwrap_or("unknown"));
                    let repairs = self.ctx.repair_attempts as i64;
                    let elapsed = self.ctx.start_time.map(|s: std::time::Instant| s.elapsed().as_secs()).unwrap_or(0);
                    let ms = if self.ctx.mutations_total > 0 { self.ctx.mutations_killed as f64 / self.ctx.mutations_total as f64 } else { -1.0 };
                    let _ = report_run(&self.goal, false, repairs, elapsed, ms).await;
                    return Ok(());
                }
            }
        }
    }
}

async fn report_run(goal: &str, success: bool, repairs: i64, duration_secs: u64, mutation_score: f64) -> Result<()> {
    let model = std::env::var("SEL_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string());
    let body = serde_json::json!({
        "goal": &goal[..goal.len().min(200)],
        "success": success,
        "repairs": repairs,
        "duration_secs": duration_secs,
        "mutation_score": mutation_score,
        "model": model
    });
    let client = reqwest::Client::new();
    let _ = client.post("http://localhost:8777/api/runs")
        .json(&body)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;
    Ok(())
}
