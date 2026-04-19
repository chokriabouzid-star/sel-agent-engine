# SEL Agent Source Code

Generated on: 2026-04-19

## File: agent.rs

```rust
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
    ctx: ExecutionContext,
    executor: SafeExecutor,
    llm: LlmEngine,
    goal: String,
    plan: Vec<Cmd>,
    previous_error: Option<String>,
    repair_fingerprints: Vec<u64>, // Repair History Guard v1.2
    context_config: ContextConfig,
    failure_memory: crate::memory::FailureMemory, // v5.8
    pub accumulated_stats: crate::llm_engine::LlmCallStats, // v6.1
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
        }
    }
    pub fn new_with_model(
        _api_key: String,
        _model_alias: String,
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
        }
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
        let _model = std::env::var("SEL_MODEL")
            .unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string());
        let body = serde_json::json!({
            "event_type": event_type,
            "goal": &self.goal,
            "step": step.unwrap_or(""),
            "detail": detail.unwrap_or(""),
            "success": success,
            "mutation": mutation,
            "repairs": self.ctx.repair_attempts,
            "model": "auto",  // provider managed by LlmEngine
            "timestamp": ""
        });
        let url = std::env::var("SEL_OBSERVATORY")
            .unwrap_or_else(|_| "http://localhost:8777".to_string());
        let _ = std::process::Command::new("curl")
            .args([
                "-s",
                "-X",
                "POST",
                &format!("{}/api/event", url),
                "-H",
                "Content-Type: application/json",
                "-d",
                &body.to_string(),
            ])
            .output();
    }

    pub fn mutation_score(&self) -> f64 {
        if self.ctx.mutations_total > 0 {
            self.ctx.mutations_killed as f64 / self.ctx.mutations_total as f64
        } else {
            -1.0
        }
    }

    // ══════════════════════════════════════════════════════════
    // Goal Validator v1.2
    fn validate_goal(goal: &str) -> Option<String> {
        let g = goal.to_lowercase();
        let len = goal.trim().len();
        if len < 10 {
            return Some("Goal too short.".to_string());
        }
        let real_keywords = [
            "fix",
            "implement",
            "refactor",
            "update",
            "migrate",
            "failing",
            "crate",
            "existing",
            "workspace",
        ];
        if real_keywords.iter().any(|kw| g.contains(kw)) {
            return None;
        }
        let has_test = g.contains("test")
            || g.contains("pytest")
            || g.contains("assert")
            || g.contains("spec")
            || g.contains("verify");
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

    // ══════════════════════════════════════════════════════════
    // v5.6: Unique Patch Enforcer
    fn build_lang_hint(&self) -> String {
        let ws = &self.executor.workspace;
        if ws.join("Cargo.toml").exists() {
            "\nCRITICAL: This is a RUST project (Cargo.toml exists). Write ONLY Rust code. Do NOT create Python or JS files.".to_string()
        } else if ws.join("package.json").exists() {
            "\nCRITICAL: This is a Node.js project (package.json exists). Write ONLY JS/TS code."
                .to_string()
        } else if ws.join("go.mod").exists() {
            "\nCRITICAL: This is a Go project (go.mod exists). Write ONLY Go code.".to_string()
        } else {
            String::new()
        }
    }

    fn build_skeleton_context(&self) -> String {
        let ws = &self.executor.workspace;
        let mut map = String::new();
        // v5.8.1: أضف محتوى Cargo.toml دائماً في Planning
        if let Ok(toml) = std::fs::read_to_string(ws.join("Cargo.toml")) {
            map.push_str(&format!(
                "CURRENT Cargo.toml CONTENT (use patch_file with EXACT text):\n```\n{}\n```\n\n",
                toml.trim()
            ));
        }
        // v5.8.2: أضف محتوى src/lib.rs دائماً في Planning
        if let Ok(lib) = std::fs::read_to_string(ws.join("src/lib.rs")) {
            map.push_str(&format!(
                "CURRENT src/lib.rs CONTENT (use patch_file with EXACT text):\n```\n{}\n```\n\n",
                lib.trim()
            ));
        }
        if let Ok(toml) = std::fs::read_to_string(ws.join("Cargo.toml")) {
            if let Some(name) = toml
                .lines()
                .find(|l| l.trim().starts_with("name"))
                .and_then(|l| l.split('"').nth(1))
            {
                map.push_str(&format!("CRATE NAME: {}\n", name));
                map.push_str(&format!("TEST IMPORT: use {}::\n\n", name));
            }
        }
        let src_dir = ws.join("src");
        if src_dir.exists() {
            let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&src_dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "rs").unwrap_or(false))
                .collect();
            files.sort();
            for path in files {
                let rel = path
                    .strip_prefix(ws)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                if let Ok(src) = std::fs::read_to_string(&path) {
                    let skeleton: Vec<String> = src
                        .lines()
                        .filter(|l| {
                            let t = l.trim();
                            t.starts_with("pub struct ")
                                || t.starts_with("pub enum ")
                                || t.starts_with("pub fn ")
                                || t.starts_with("fn ")
                                || t.starts_with("pub mod ")
                                || t.starts_with("mod ")
                                || t.starts_with("pub use ")
                                || t.starts_with("impl ")
                        })
                        .map(|l| {
                            let t = l.trim();
                            let sig = if t.contains('{') {
                                t.splitn(2, '{').next().unwrap_or(t).trim().to_string() + " { ... }"
                            } else {
                                t.to_string()
                            };
                            format!("  {}", sig)
                        })
                        .collect();
                    if !skeleton.is_empty() {
                        map.push_str(&format!("FILE: {}\n{}\n\n", rel, skeleton.join("\n")));
                    }
                }
            }
        }
        if !map.is_empty() {
            map.push_str("CRITICAL RULES (violations = build failure):\n");
            map.push_str("- NEVER use write_file on existing files — use patch_file only\n");
            map.push_str("- NEVER redefine functions already listed above\n");
            map.push_str(
                "- NEVER guess the crate name — use exactly what CRATE NAME shows above\n",
            );
        }
        map
    }

    // v6.6: Auto-Context Injection — يقرأ كل ملفات الـ workspace الموجودة
    fn build_workspace_context(&self) -> String {
        let ws = &self.executor.workspace;
        let mut ctx = String::new();

        // الامتدادات المدعومة
        let supported = ["ts", "js", "py", "go", "rs", "toml", "json", "mod"];

        // اقرأ كل الملفات بشكل recursive (حد 50 ملف، حد 300 سطر لكل ملف)
        let mut files: Vec<std::path::PathBuf> = walkdir::WalkDir::new(ws)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
            .map(|e| e.path().to_path_buf())
            .filter(|p| p.is_file())
            .filter(|p| {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                supported.contains(&ext)
            })
            .filter(|p| {
                // تجاهل node_modules, venv, dist, target
                let s = p.to_string_lossy();
                !s.contains("node_modules")
                    && !s.contains("/venv/")
                    && !s.contains("/dist/")
                    && !s.contains("/target/")
                    && !s.contains("/.")
                    && !s.contains("package-lock")
            })
            .take(50)
            .collect();
        files.sort();

        if files.is_empty() {
            return String::new();
        }

        ctx.push_str("=== EXISTING WORKSPACE FILES (read carefully before planning) ===\n");
        ctx.push_str("CRITICAL: Use patch_file (NOT write_file) for ALL files listed below.\n\n");

        // سقف صارم: 4000 token إجمالي للـ context (حوالي 16000 حرف)
        const MAX_CONTEXT_CHARS: usize = 16_000;
        let mut total_chars = 0usize;

        for path in &files {
            if total_chars >= MAX_CONTEXT_CHARS {
                ctx.push_str("... (remaining files omitted — context limit reached)\n");
                break;
            }
            let rel = path.strip_prefix(ws).unwrap_or(path).to_string_lossy();
            if let Ok(src) = std::fs::read_to_string(path) {
                let lines: Vec<&str> = src.lines().collect();
                // حد 60 سطر لكل ملف بدل 300
                let max_lines = 60usize;
                let preview: Vec<&str> = lines.iter().take(max_lines).cloned().collect();
                let file_content = format!(
                    "--- FILE: {} ({} lines) ---\n{}\n{}\n",
                    rel,
                    lines.len(),
                    preview.join("\n"),
                    if lines.len() > max_lines {
                        format!("... ({} more lines)", lines.len() - max_lines)
                    } else {
                        String::new()
                    }
                );
                // لا تضف إذا سيتجاوز الحد
                if total_chars + file_content.len() > MAX_CONTEXT_CHARS {
                    ctx.push_str(&format!(
                        "--- FILE: {} (skipped — context limit) ---\n\n",
                        rel
                    ));
                    break;
                }
                total_chars += file_content.len();
                ctx.push_str(&file_content);
            }
        }

        ctx.push_str("=== END OF EXISTING FILES ===\n\n");
        ctx
    }

    fn build_ref_context(&self) -> String {
        if let Some(ref ref_path) = self.context_config.ref_file {
            crate::context::read_ref_file(ref_path)
                .map(|s| format!("\nREFERENCE FILE (use exact signatures):\n{}\n", s))
                .unwrap_or_default()
        } else {
            String::new()
        }
    }

    fn validate_patch_uniqueness(&self, plan: &[Cmd]) -> Vec<String> {
        let mut issues = Vec::new();
        for cmd in plan {
            if let Cmd::PatchFile { path, search, .. } = cmd {
                let full_path = self.executor.workspace.join(path);
                if !full_path.exists() {
                    continue;
                }
                let content = match std::fs::read_to_string(&full_path) {
                    Ok(c) => c,
                    Err(e) => {
                        issues.push(format!("Could not read '{}': {}", path, e));
                        continue;
                    }
                };
                let count = content.matches(search.as_str()).count();
                if count == 0 {
                    issues.push(format!(
                        "search block not found in '{}' — copy text VERBATIM from the file",
                        path
                    ));
                } else if count > 1 {
                    issues.push(format!(
                        "search block found {} times in '{}' — add more surrounding context lines",
                        count, path
                    ));
                }
            }
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
        let existing_files = if self.context_config.ref_file.is_some() {
            self.build_skeleton_context()
        } else {
            String::new()
        };
        let ref_context = self.build_ref_context();
        let lang_hint = self.build_lang_hint();
        let prompt = crate::constitution::CONSTITUTION.to_string() + &format!(
            "{}{}{}\nGoal: {}\n\nFEEDBACK:\n{}",
            existing_files, ref_context, lang_hint, self.goal, feedback
        );
        match self.plan_with_resilience(prompt).await {
            Ok(new_plan) => {
                let new_issues = self.validate_patch_uniqueness(&new_plan);
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
                    // Goal Validator v1.2 — يرفض الـ goal الغامض صامتاً
                    if let Some(reason) = Self::validate_goal(&self.goal) {
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
                    // v5.6: استخدام helpers المستخرجة
                    let lang_hint = self.build_lang_hint();
                    // v5.6: استخدام build_skeleton_context helper
                    // v6.6: Auto-Context Injection
                    // استخدم workspace context إذا كانت هناك ملفات موجودة
                    let ws_ctx = self.build_workspace_context();
                    let existing_files = if !ws_ctx.is_empty() {
                        ws_ctx
                    } else if self.context_config.ref_file.is_some() {
                        self.build_skeleton_context()
                    } else {
                        String::new()
                    };

                    // v5.6: استخدام build_ref_context helper
                    let ref_context = self.build_ref_context();

                    let prompt = crate::constitution::CONSTITUTION.to_string() + &format!(
                        "{}{}{}\n{}\n{}\nGoal: {}\nProvide the complete execution plan.",
                        existing_files, ref_context, lang_hint, env_context, constraints, self.goal
                    );
                    // Protocol Resilience v1.3
                    match self.plan_with_resilience(prompt).await {
                        Ok(commands) => {
                            println!("   ✓ {} commands", commands.len());
                            // v5.6: Unique Patch Enforcer — التحقق قبل التنفيذ
                            let issues = self.validate_patch_uniqueness(&commands);
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
                                let impl_source = self.plan.iter().find_map(|c| match c {
                                    crate::protocol::Cmd::WriteFile { path, .. }
                                        if (path.ends_with(".py")
                                            || path.ends_with(".go")
                                            || path.ends_with(".js")
                                            || path.ends_with(".ts")
                                            || path.ends_with(".rs"))
                                            && !path.contains("test")
                                            && !path.contains("Cargo.toml")
                                            && !path.contains("go.mod")
                                            && !path.contains("package.json")
                                            && !path.contains("jest.config")
                                            && !path.contains("tsconfig") =>
                                    {
                                        Some(path.clone())
                                    }
                                    _ => None,
                                });
                                let mut mutation_passed = true;
                                if !self.ctx.skip_mutation {
                                if let Some(src) = impl_source {
                                    use crate::executor::MutationResult;
                                    println!("\n🧬 Mutation check on {}...", src);
                                    match self.executor.mutation_check(&src).await {
                                        MutationResult::Weak(orig_line, mutd_line) => {
                                            self.ctx.mutations_total += 1;
                                            println!("   ⚠️  Tests are WEAK — triggering repair (Mutation Enforcement v1.3).");
                                            println!(
                                                "     Survived mutation: [{}] → [{}]",
                                                orig_line, mutd_line
                                            );
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
                                        MutationResult::Strong => {
                                            println!("   ✅ Tests are solid.");
                                            self.send_event(
                                                "mutation",
                                                None,
                                                None,
                                                None,
                                                Some(1.0),
                                            );
                                            self.ctx.mutations_total += 1;
                                            self.ctx.mutations_killed += 1;
                                        }
                                        MutationResult::Skipped => {
                                            println!("   ⏭  Mutation check skipped.")
                                        }
                                    }
                                }
                                } // end skip_mutation guard
                                // ─────────────────────────────
                                if mutation_passed {
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
                                } else {
                                    self.state = AgentState::Repairing;
                                }
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
                            let py_source = self.plan.iter().find_map(|c| match c {
                                crate::protocol::Cmd::WriteFile { path, .. }
                                    if path.ends_with(".py") && !path.contains("test") =>
                                {
                                    Some(path.clone())
                                }
                                _ => None,
                            });
                            if let Some(src) = py_source {
                                use crate::executor::MutationResult;
                                println!("\n🧬 Mutation check on {}...", src);
                                match self.executor.mutation_check(&src).await {
                                    MutationResult::Weak(orig_line, mutd_line) => {
                                        println!("   ⚠️  Tests are WEAK — triggering repair (Mutation Enforcement v1.3).");
                                        println!(
                                            "     Survived mutation: [{}] → [{}]",
                                            orig_line, mutd_line
                                        );
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
                                    MutationResult::Strong => {
                                        self.ctx.mutations_total += 1;
                                        self.ctx.mutations_killed += 1;
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
                    let _patch_error_context: String = {
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
                    let _loop_warning = if self.repair_fingerprints.len() > 1
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
                        crate::context::read_ref_file(&ref_path)
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
                        self.goal, network_note, mutation_note, patch_note, ref_file_context, memory_hint, repair_hint, attempt_note, errors, files_context, _patch_error_context
                    );

                    // Protocol Resilience v1.3
                    match self.plan_with_resilience(prompt).await {
                        Ok(commands) => {
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
                    return Ok(());
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

```

## File: bench_compile.rs

```rust
// src/bench_compile.rs — SEL Bench Compile Suite v1.1
// إصلاحات: operator precedence, content verification, mutation requirement

use std::path::Path;

pub struct CompileCase {
    pub name: &'static str,
    pub lang: &'static str,
    pub goal: &'static str,
    pub max_repairs: u8,
    pub require_mutation: bool,
}

pub struct CompileCheck {
    pub passed: bool,
    pub repairs: usize,
    pub created_wrong_files: bool,
    pub mutation_ok: bool,
    pub notes: Vec<String>,
}

pub fn setup_case(name: &str, ws: &Path) {
    match name {
        "go_undefined_import" => setup_go_undefined_import(ws),
        "go_unescaped_quotes" => setup_go_unescaped_quotes(ws),
        "go_wrong_logic" => setup_go_wrong_logic(ws),
        "python_module_missing" => setup_python_module_missing(ws),
        "python_wrong_logic" => setup_python_wrong_logic(ws),
        "python_wrong_import" => setup_python_wrong_import(ws),
        "go_syntax_error" => setup_go_syntax_error(ws),
        "python_syntax_then_logic" => setup_python_syntax_then_logic(ws),
        _ => {}
    }
}

pub fn check_result(name: &str, ws: &Path, ok: bool, repairs: usize, mutation: f64) -> CompileCheck {
    let mut result = CompileCheck {
        passed: ok,
        repairs,
        created_wrong_files: false,
        mutation_ok: true,
        notes: Vec::new(),
    };

    // فحص عام: لم ينشئ ملفات من لغة خاطئة
    match name {
        n if n.starts_with("go_") => {
            if path_exists(ws, "*.py") || path_exists(ws, "test_*.py") {
                result.created_wrong_files = true;
                result.passed = false;
                result.notes.push("created .py files in Go project".into());
            }
        }
        n if n.starts_with("python_") => {
            if path_exists(ws, "*.go") {
                result.created_wrong_files = true;
                result.passed = false;
                result.notes.push("created .go files in Python project".into());
            }
        }
        _ => {}
    }

    // فحص خاص لكل حالة — يقرأ الملف النهائي ويتحقق
    match name {
        "go_undefined_import" => {
            if repairs > 1 {
                result.passed = false;
                result.notes.push(format!("too many repairs: {}", repairs));
            }
            match std::fs::read_to_string(ws.join("main.go")) {
                Ok(content) => {
                    if !content.contains("\"fmt\"") {
                        result.passed = false;
                        result.notes.push("missing import \"fmt\"".into());
                    }
                    if !content.contains("func Hello()") {
                        result.passed = false;
                        result.notes.push("Hello function removed".into());
                    }
                }
                Err(_) => {
                    result.passed = false;
                    result.notes.push("main.go not found".into());
                }
            }
        }

        "go_unescaped_quotes" => {
            if path_exists(ws, "calc.go") || path_exists(ws, "calc_test.go") {
                result.created_wrong_files = true;
                result.passed = false;
                result.notes.push("created unexpected files".into());
            }
            // تحقق أن main.go لم يتغير (الخطأ في test فقط)
            match std::fs::read_to_string(ws.join("main.go")) {
                Ok(content) => {
                    if !content.contains("func Reverse(s string) string") {
                        result.passed = false;
                        result.notes.push("Reverse function was modified".into());
                    }
                }
                Err(_) => {}
            }
        }

        "go_wrong_logic" => {
            match std::fs::read_to_string(ws.join("main.go")) {
                Ok(content) => {
                    // Add يجب أن يكون a + b
                    let has_correct_add = content.contains("a + b")
                        && content.lines().any(|l| {
                            l.contains("Add") && l.contains("func")
                                || (l.contains("return") && l.contains("a + b")
                                    && !l.contains("Multiply"))
                        });

                    // Multiply يجب أن يكون a * b
                    let has_correct_multiply = content.contains("a * b");

                    // لا يزال فيه الأخطاء القديمة؟
                    let still_has_subtract = content.lines().any(|l| {
                        l.contains("a - b")
                    });

                    if still_has_subtract {
                        result.passed = false;
                        result.notes.push("Add still uses a - b".into());
                    }
                    if !has_correct_multiply {
                        result.passed = false;
                        result.notes.push("Multiply not fixed to a * b".into());
                    }
                    if !has_correct_add && !still_has_subtract {
                        // تحقق إضافي
                        result.notes.push("Add implementation unclear".into());
                    }
                }
                Err(_) => {
                    result.passed = false;
                    result.notes.push("main.go not found".into());
                }
            }
            // mutation مطلوب
            if mutation < 0.0 {
                result.notes.push("mutation not measured".into());
                // لا نفشّله لهذا — لكن نسجل
            } else if mutation < 1.0 {
                result.mutation_ok = false;
                result.passed = false;
                result.notes.push(format!("mutation {:.0}% < 100%", mutation * 100.0));
            }
        }

        "python_module_missing" => {
            match std::fs::read_to_string(ws.join("app.py")) {
                Ok(content) => {
                    if !content.contains("import requests") && !content.contains("from requests") {
                        result.passed = false;
                        result.notes.push("import requests was removed".into());
                    }
                }
                Err(_) => {}
            }
        }

        "python_wrong_logic" => {
            match std::fs::read_to_string(ws.join("calculator.py")) {
                Ok(content) => {
                    // divide يجب أن يستخدم / أو //
                    let has_divide_op = content.lines().any(|l| {
                        l.contains("return") && (l.contains("a / b") || l.contains("a // b"))
                            && !l.contains("a * b")
                    });
                    // power يجب أن يستخدم **
                    let has_power_op = content.contains("**");

                    if !has_divide_op {
                        result.passed = false;
                        result.notes.push("divide not fixed".into());
                    }
                    if !has_power_op {
                        result.passed = false;
                        result.notes.push("power not using **".into());
                    }
                }
                Err(_) => {}
            }
        }

        "python_wrong_import" => {
            match std::fs::read_to_string(ws.join("test_models.py")) {
                Ok(content) => {
                    if content.contains("wrong_module") {
                        result.passed = false;
                        result.notes.push("still imports from wrong_module".into());
                    }
                    if !content.contains("from models") {
                        result.passed = false;
                        result.notes.push("not importing from models".into());
                    }
                }
                Err(_) => {}
            }
            // models.py يجب أن لا يتغير
            match std::fs::read_to_string(ws.join("models.py")) {
                Ok(content) => {
                    if !content.contains("class User:") {
                        result.passed = false;
                        result.notes.push("models.py was incorrectly modified".into());
                    }
                }
                Err(_) => {}
            }
        }

        "go_syntax_error" => {
            if repairs > 2 {
                result.passed = false;
                result.notes.push(format!("too many repairs: {}", repairs));
            }
            // تحقق أن Fibonacci صالحة
            match std::fs::read_to_string(ws.join("main.go")) {
                Ok(content) => {
                    let open_braces = content.matches('{').count();
                    let close_braces = content.matches('}').count();
                    if open_braces != close_braces {
                        result.passed = false;
                        result.notes.push("unbalanced braces".into());
                    }
                }
                Err(_) => {}
            }
        }

        "python_syntax_then_logic" => {
            if repairs > 2 {
                result.passed = false;
                result.notes.push(format!("too many repairs: {}", repairs));
            }
            match std::fs::read_to_string(ws.join("processor.py")) {
                Ok(content) => {
                    // لا syntax errors
                    if content.contains("== 0\n") && !content.contains("== 0:") {
                        result.passed = false;
                        result.notes.push("missing colon after if".into());
                    }
                    if content.contains("len(items\n") {
                        result.passed = false;
                        result.notes.push("unclosed paren".into());
                    }
                }
                Err(_) => {}
            }
        }

        _ => {}
    }

    // إذا لا ملاحظات والنتيجة ناجحة
    if result.notes.is_empty() && result.passed {
        result.notes.push("ok".into());
    }

    result
}

pub fn all_cases() -> Vec<CompileCase> {
    vec![
        CompileCase {
            name: "go_undefined_import",
            lang: "go",
            goal: "Fix the code so tests pass. The main.go uses fmt but doesn't import it. Add the missing import. Run go test.",
            max_repairs: 3,
            require_mutation: false,
        },
        CompileCase {
            name: "go_unescaped_quotes",
            lang: "go",
            goal: "Fix the test file main_test.go — it has unescaped quotes in the Errorf call. Fix ONLY the test file. Do NOT create new files. Run go test.",
            max_repairs: 3,
            require_mutation: false,
        },
        CompileCase {
            name: "go_wrong_logic",
            lang: "go",
            goal: "Fix main.go: Add should return a+b (not a-b), Multiply should return a*b (not a+b). Do NOT modify tests. Run go test.",
            max_repairs: 3,
            require_mutation: true,
        },
        CompileCase {
            name: "python_module_missing",
            lang: "python",
            goal: "Install the missing requests module and make tests pass. Do NOT remove the import. Run pytest.",
            max_repairs: 3,
            require_mutation: false,
        },
        CompileCase {
            name: "python_wrong_logic",
            lang: "python",
            goal: "Fix calculator.py: divide should return a/b (not a*b), power should return base**exp (not base+exp). Do NOT modify tests. Run pytest.",
            max_repairs: 3,
            require_mutation: true,
        },
        CompileCase {
            name: "python_wrong_import",
            lang: "python",
            goal: "Fix test_models.py: it imports from wrong_module but should import from models. Fix ONLY the import. Do NOT modify models.py. Run pytest.",
            max_repairs: 3,
            require_mutation: false,
        },
        CompileCase {
            name: "go_syntax_error",
            lang: "go",
            goal: "Fix main.go: there is a missing closing brace in the Fibonacci function. Fix the syntax error. Run go test.",
            max_repairs: 3,
            require_mutation: false,
        },
        CompileCase {
            name: "python_syntax_then_logic",
            lang: "python",
            goal: "Fix processor.py: it has syntax errors (missing colon and closing paren). Fix syntax first, then ensure logic handles empty list correctly. Run pytest.",
            max_repairs: 3,
            require_mutation: false,
        },
    ]
}

// ══════════════════════════════════════════════════════
// Setup functions
// ══════════════════════════════════════════════════════

fn setup_go_undefined_import(ws: &Path) {
    let _ = std::fs::write(ws.join("main.go"), "package main\n\nfunc Hello() string {\n\treturn fmt.Sprintf(\"hello\")\n}\n");
    let _ = std::fs::write(ws.join("main_test.go"), "package main\n\nimport \"testing\"\n\nfunc TestHello(t *testing.T) {\n\tif Hello() != \"hello\" {\n\t\tt.Errorf(\"got %q\", Hello())\n\t}\n}\n");
    go_mod_init(ws);
}

fn setup_go_unescaped_quotes(ws: &Path) {
    let _ = std::fs::write(ws.join("main.go"), "package main\n\nfunc Reverse(s string) string {\n\trunes := []rune(s)\n\tfor i, j := 0, len(runes)-1; i < j; i, j = i+1, j-1 {\n\t\trunes[i], runes[j] = runes[j], runes[i]\n\t}\n\treturn string(runes)\n}\n");
    let test_content = b"package main\n\nimport \"testing\"\n\nfunc TestReverse(t *testing.T) {\n\tgot := Reverse(\"hello\")\n\tif got != \"olleh\" {\n\t\tt.Errorf(\"Reverse(\"hello\") = %q, want olleh\", got)\n\t}\n}\n";
    let _ = std::fs::write(ws.join("main_test.go"), test_content);
    go_mod_init(ws);
}

fn setup_go_wrong_logic(ws: &Path) {
    let _ = std::fs::write(ws.join("main.go"), "package main\n\nfunc Add(a, b int) int      { return a - b }\nfunc Multiply(a, b int) int { return a + b }\n");
    let _ = std::fs::write(ws.join("main_test.go"), "package main\n\nimport \"testing\"\n\nfunc TestAdd(t *testing.T) {\n\tif got := Add(2, 3); got != 5 {\n\t\tt.Errorf(\"Add(2,3) = %d, want 5\", got)\n\t}\n\tif got := Add(0, 0); got != 0 {\n\t\tt.Errorf(\"Add(0,0) = %d, want 0\", got)\n\t}\n}\n\nfunc TestMultiply(t *testing.T) {\n\tif got := Multiply(3, 4); got != 12 {\n\t\tt.Errorf(\"Multiply(3,4) = %d, want 12\", got)\n\t}\n\tif got := Multiply(0, 5); got != 0 {\n\t\tt.Errorf(\"Multiply(0,5) = %d, want 0\", got)\n\t}\n}\n");
    go_mod_init(ws);
}

fn setup_python_module_missing(ws: &Path) {
    let _ = std::fs::write(ws.join("app.py"), "import requests\n\ndef fetch(url):\n    return requests.get(url).status_code\n");
    let _ = std::fs::write(ws.join("test_app.py"), "from app import fetch\n\ndef test_fetch_callable():\n    assert callable(fetch)\n\ndef test_fetch_type():\n    assert fetch.__name__ == \"fetch\"\n");
}

fn setup_python_wrong_logic(ws: &Path) {
    let _ = std::fs::write(ws.join("calculator.py"), "def divide(a, b):\n    return a * b\n\ndef power(base, exp):\n    return base + exp\n");
    let _ = std::fs::write(ws.join("test_calculator.py"), "from calculator import divide, power\n\ndef test_divide():\n    assert divide(10, 2) == 5\n    assert divide(9, 3) == 3\n    assert divide(0, 5) == 0\n\ndef test_power():\n    assert power(2, 3) == 8\n    assert power(3, 2) == 9\n    assert power(5, 0) == 1\n");
}

fn setup_python_wrong_import(ws: &Path) {
    let _ = std::fs::write(ws.join("models.py"), "class User:\n    def __init__(self, name, email):\n        self.name = name\n        self.email = email\n\n    def greet(self):\n        return f\"Hello, {self.name}\"\n");
    let _ = std::fs::write(ws.join("test_models.py"), "from wrong_module import User\n\ndef test_user_creation():\n    u = User(\"Alice\", \"alice@example.com\")\n    assert u.name == \"Alice\"\n    assert u.email == \"alice@example.com\"\n\ndef test_user_greet():\n    u = User(\"Bob\", \"bob@example.com\")\n    assert u.greet() == \"Hello, Bob\"\n");
}

fn setup_go_syntax_error(ws: &Path) {
    let _ = std::fs::write(ws.join("main.go"), "package main\n\nfunc Fibonacci(n int) int {\n\tif n <= 1 {\n\t\treturn n\n\t\n\treturn Fibonacci(n-1) + Fibonacci(n-2)\n}\n");
    let _ = std::fs::write(ws.join("main_test.go"), "package main\n\nimport \"testing\"\n\nfunc TestFibonacci(t *testing.T) {\n\ttests := []struct{ n, want int }{\n\t\t{0, 0}, {1, 1}, {5, 5}, {10, 55},\n\t}\n\tfor _, tt := range tests {\n\t\tif got := Fibonacci(tt.n); got != tt.want {\n\t\t\tt.Errorf(\"Fibonacci(%d) = %d, want %d\", tt.n, got, tt.want)\n\t\t}\n\t}\n}\n");
    go_mod_init(ws);
}

fn setup_python_syntax_then_logic(ws: &Path) {
    let _ = std::fs::write(ws.join("processor.py"), "def process(items):\n    if len(items) == 0\n        return 0\n    return sum(items) / len(items\n");
    let _ = std::fs::write(ws.join("test_processor.py"), "from processor import process\n\ndef test_normal():\n    assert process([1, 2, 3, 4, 5]) == 3.0\n    assert process([10, 20]) == 15.0\n\ndef test_edge():\n    assert process([]) == 0\n    assert process([42]) == 42.0\n");
}

fn go_mod_init(ws: &Path) {
    let _ = std::process::Command::new("go")
        .args(["mod", "init", "gotest"])
        .current_dir(ws)
        .output();
}

fn path_exists(ws: &Path, pattern: &str) -> bool {
    if let Ok(entries) = std::fs::read_dir(ws) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if pattern.starts_with("*.") {
                let ext = &pattern[1..];
                if name.ends_with(ext) {
                    return true;
                }
            } else if pattern.starts_with("test_*.") {
                let ext = &pattern[6..];
                if name.starts_with("test_") && name.ends_with(ext) {
                    return true;
                }
            } else if name == pattern {
                return true;
            }
        }
    }
    false
}

```

## File: bench_realworld.rs

```rust
// src/bench_realworld.rs — v7.3 Feature-Targeted Benchmark
// يختبر: Compile-First | quick_fix | Language Guard | Real-World Patterns

use crate::agent;
use crate::types;
use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::{Duration, Instant};

struct BenchCase {
    name: &'static str,
    goal: &'static str,
    lang: &'static str,
    tier: u8,
    tests_feature: &'static str,
    reference_tests: Option<(&'static str, &'static str)>,
    scaffold_files: Vec<(&'static str, &'static str)>,
}

impl BenchCase {
    fn new(
        name: &'static str,
        goal: &'static str,
        lang: &'static str,
        tier: u8,
        tests_feature: &'static str,
    ) -> Self {
        Self {
            name,
            goal,
            lang,
            tier,
            tests_feature,
            reference_tests: None,
            scaffold_files: vec![],
        }
    }

    fn with_tests(mut self, filename: &'static str, content: &'static str) -> Self {
        self.reference_tests = Some((filename, content));
        self
    }

    fn with_scaffold(mut self, path: &'static str, content: &'static str) -> Self {
        self.scaffold_files.push((path, content));
        self
    }
}

pub async fn run_bench_realworld(
    _api_key: &str,
    tier: Option<u8>,
    max_repairs: u8,
) -> Result<()> {
    println!("\n╔═══════════════════════════════════════════════════════════════════╗");
    println!("║   SEL Agent v7.3.0 — Feature-Targeted Benchmark                   ║");
    println!("║   Compile-First | quick_fix | Language Guard | Real-World         ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝\n");

    let all_cases = build_cases();

    let cases: Vec<_> = if let Some(t) = tier {
        all_cases.into_iter().filter(|c| c.tier == t).collect()
    } else {
        all_cases
    };

    if cases.is_empty() {
        println!("No cases for tier {:?}", tier);
        return Ok(());
    }

    print_test_plan(&cases);

    let total = cases.len();
    let mut passed = 0usize;
    let mut total_repairs = 0usize;
    let mut feature_stats: std::collections::HashMap<&str, (usize, usize)> =
        std::collections::HashMap::new();

    let start_time = Instant::now();
    let tmpdir = std::env::temp_dir();

    for (i, case) in cases.iter().enumerate() {
        let workspace = tmpdir.join(format!("sel-bench-v73-{}-{}", i, std::process::id()));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace)?;

        // كتابة reference tests
        if let Some((filename, content)) = case.reference_tests {
            let test_path = workspace.join(filename);
            if let Some(parent) = test_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&test_path, content)?;
        }

        // كتابة scaffold files
        for (path, content) in &case.scaffold_files {
            let full_path = workspace.join(path);
            if let Some(parent) = full_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&full_path, content)?;
        }

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!(
                    "{{spinner:.yellow}} [{}/{}] T{} [{}] {}...",
                    i + 1,
                    total,
                    case.tier,
                    case.lang,
                    case.name
                ))
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.enable_steady_tick(Duration::from_millis(80));

        let mut ag = agent::Agent::new(
            String::new(),
            workspace.clone(),
            case.goal.to_string(),
            max_repairs,
            types::ContextConfig::default(),
        );

        let case_start = Instant::now();
        let result = ag.run().await;
        let case_dur = case_start.elapsed();
        pb.finish_and_clear();

        let entry = feature_stats
            .entry(case.tests_feature)
            .or_insert((0, 0));
        entry.1 += 1;

        match result {
            Ok(_) => {
                let repairs = ag.repair_count();
                if ag.is_success() {
                    total_repairs += repairs;
                    passed += 1;
                    entry.0 += 1;
                    println!(
                        "  {} T{} [{}] {} ({}s, {} repairs) | {}",
                        "✅".green(),
                        case.tier,
                        case.lang.blue(),
                        case.name.bold(),
                        case_dur.as_secs(),
                        repairs,
                        case.tests_feature.magenta()
                    );
                } else {
                    println!(
                        "  {} T{} [{}] {} ({}s, {} repairs) | {}",
                        "❌".red(),
                        case.tier,
                        case.lang.blue(),
                        case.name.bold(),
                        case_dur.as_secs(),
                        repairs,
                        case.tests_feature.magenta()
                    );
                }
            }
            Err(e) => {
                println!(
                    "  {} T{} [{}] {} ({}s) | ERR: {}",
                    "💥".red(),
                    case.tier,
                    case.lang.blue(),
                    case.name.bold(),
                    case_dur.as_secs(),
                    e.to_string().chars().take(80).collect::<String>()
                );
            }
        }

        let _ = std::fs::remove_dir_all(&workspace);

        if i < total - 1 {
            println!("     ⏳ 15s cooldown...");
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    }

    print_results(
        passed,
        total,
        total_repairs,
        start_time.elapsed(),
        &feature_stats,
        tier,
    );

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Cases — مصممة لاختبار ميزات v7.3
// ═══════════════════════════════════════════════════════════════════════════

fn build_cases() -> Vec<BenchCase> {
    vec![
        // ────────────────────────────────────────────────────────────
        // TIER 1: Compile-First Pipeline
        // هدف: التحقق أن compile check يعمل لكل لغة
        // ────────────────────────────────────────────────────────────
        BenchCase::new(
            "Compile-Check Python",
            "Create a Python module 'calculator.py' with functions: add(a,b), subtract(a,b), multiply(a,b), divide(a,b). divide must raise ValueError if b is zero.\n\nTests: python -m pytest test_calculator.py -v",
            "Python",
            1,
            "py_compile",
        )
        .with_tests(
            "test_calculator.py",
            r#"import pytest
from calculator import add, subtract, multiply, divide

def test_add():
    assert add(2, 3) == 5
    assert add(-1, 1) == 0

def test_subtract():
    assert subtract(5, 3) == 2
    assert subtract(0, 5) == -5

def test_multiply():
    assert multiply(3, 4) == 12
    assert multiply(0, 100) == 0

def test_divide():
    assert divide(10, 2) == 5.0
    assert divide(7, 2) == 3.5

def test_divide_by_zero():
    with pytest.raises(ValueError):
        divide(5, 0)
"#,
        ),

        BenchCase::new(
            "Compile-Check Go",
            "Create a Go package 'mathutil' with exported functions: Add, Subtract, Multiply, Divide. Divide returns (float64, error) and returns error if b is zero. Package name must be 'mathutil'.
Tests: go test -v",
            "Go",
            1,
            "go test -run=^$",
        )
        .with_tests(
            "mathutil_test.go",
            r#"package mathutil

import "testing"

func TestAdd(t *testing.T) {
    if Add(2, 3) != 5 { t.Error("Add(2,3) expected 5") }
    if Add(-1, 1) != 0 { t.Error("Add(-1,1) expected 0") }
}

func TestSubtract(t *testing.T) {
    if Subtract(5, 3) != 2 { t.Error("Subtract(5,3) expected 2") }
}

func TestMultiply(t *testing.T) {
    if Multiply(3, 4) != 12 { t.Error("Multiply(3,4) expected 12") }
    if Multiply(0, 5) != 0 { t.Error("Multiply(0,5) expected 0") }
}

func TestDivide(t *testing.T) {
    r, err := Divide(10, 2)
    if err != nil || r != 5 { t.Errorf("Divide(10,2) expected 5, got %v %v", r, err) }
    _, err = Divide(5, 0)
    if err == nil { t.Error("Divide by zero must return error") }
}
"#,
        )
        .with_scaffold("go.mod", "module mathutil\n\ngo 1.21\n"),

        BenchCase::new(
            "Compile-Check Rust",
            "Create a Rust library in src/lib.rs with pub fn add(a:i32,b:i32)->i32, subtract, multiply, and divide(a:f64,b:f64)->Result<f64,String> returning Err if b is zero. Include #[cfg(test)] with tests for all four functions.\n\nTests: cargo test",
            "Rust",
            1,
            "cargo check",
        )
        .with_scaffold(
            "Cargo.toml",
            "[package]\nname = \"mathlib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
        ),

        BenchCase::new(
            "Compile-Check TypeScript",
            "Create a TypeScript file 'utils.ts' exporting: add(a:number,b:number):number, subtract, multiply, divide (throws Error if b is zero). Use explicit TypeScript types throughout.\n\nTests: npx jest utils.test.ts",
            "TypeScript",
            1,
            "tsc --noEmit",
        )
        .with_tests(
            "utils.test.ts",
            r#"import { add, subtract, multiply, divide } from './utils';

describe('utils', () => {
    test('add', () => {
        expect(add(2, 3)).toBe(5);
        expect(add(-1, 1)).toBe(0);
    });
    test('subtract', () => {
        expect(subtract(5, 3)).toBe(2);
    });
    test('multiply', () => {
        expect(multiply(3, 4)).toBe(12);
    });
    test('divide', () => {
        expect(divide(10, 2)).toBe(5);
    });
    test('divide by zero throws', () => {
        expect(() => divide(5, 0)).toThrow();
    });
});
"#,
        ),

        // ────────────────────────────────────────────────────────────
        // TIER 2: quick_fix (إصلاح تلقائي بدون LLM)
        // هدف: AutoFix Go imports + ModuleNotFoundError handling
        // ────────────────────────────────────────────────────────────
        BenchCase::new(
            "QuickFix Go imports",
            "Create a Go program in main.go with package main. Implement func Greet(name string) string that returns 'HELLO, NAME!' in uppercase using strings.ToUpper and fmt.Sprintf. The file must compile and pass the existing tests.",
            "Go",
            2,
            "quick_fix: go import",
        )
        .with_tests(
            "main_test.go",
            r#"package main

import "testing"

func TestGreet(t *testing.T) {
    result := Greet("world")
    if result != "HELLO, WORLD!" {
        t.Errorf("expected 'HELLO, WORLD!' got '%s'", result)
    }
    result2 := Greet("alice")
    if result2 != "HELLO, ALICE!" {
        t.Errorf("expected 'HELLO, ALICE!' got '%s'", result2)
    }
}
"#,
        )
        .with_scaffold("go.mod", "module greeter\n\ngo 1.21\n"),

        BenchCase::new(
            "QuickFix Python requests",
            "Create a Python module 'fetcher.py' with function fetch(url: str) -> dict that uses the 'requests' library to GET the url and returns response.json(). Handle connection errors by returning {'error': str(e)}.\n\nTests: python -m pytest test_fetcher.py -v",
            "Python",
            2,
            "quick_fix: pip install",
        )
        .with_tests(
            "test_fetcher.py",
            r#"from unittest.mock import patch, MagicMock
from fetcher import fetch

@patch('fetcher.requests.get')
def test_fetch_success(mock_get):
    mock_response = MagicMock()
    mock_response.json.return_value = {"key": "value"}
    mock_get.return_value = mock_response
    result = fetch("http://example.com")
    assert result == {"key": "value"}

@patch('fetcher.requests.get')
def test_fetch_error(mock_get):
    mock_get.side_effect = Exception("connection refused")
    result = fetch("http://bad-url")
    assert "error" in result
"#,
        ),

        BenchCase::new(
            "QuickFix Go strings package",
            "Create a Go file 'textutils.go' in package textutils with exported functions: Reverse(s string) string, CountVowels(s string) int, IsPalindrome(s string) bool. Use only standard library.\n\nTests: go test -v",
            "Go",
            2,
            "quick_fix: go strings",
        )
        .with_tests(
            "textutils_test.go",
            r#"package textutils

import "testing"

func TestReverse(t *testing.T) {
    if Reverse("hello") != "olleh" { t.Error("Reverse failed") }
    if Reverse("") != "" { t.Error("Reverse empty failed") }
    if Reverse("a") != "a" { t.Error("Reverse single failed") }
}

func TestCountVowels(t *testing.T) {
    if CountVowels("hello") != 2 { t.Error("CountVowels hello failed") }
    if CountVowels("rhythm") != 0 { t.Error("CountVowels rhythm failed") }
    if CountVowels("aeiou") != 5 { t.Error("CountVowels aeiou failed") }
}

func TestIsPalindrome(t *testing.T) {
    if !IsPalindrome("racecar") { t.Error("racecar is palindrome") }
    if !IsPalindrome("level") { t.Error("level is palindrome") }
    if IsPalindrome("hello") { t.Error("hello is not palindrome") }
}
"#,
        )
        .with_scaffold("go.mod", "module textutils\n\ngo 1.21\n"),

        // ────────────────────────────────────────────────────────────
        // TIER 3: Language Guard + Bug Fix
        // هدف: الوكيل يُصلح ملفاً موجوداً دون كسر workspace
        // ────────────────────────────────────────────────────────────
        BenchCase::new(
            "BugFix Rust: wrong operator",
            "Fix the bug in src/lib.rs. The add function currently returns a - b instead of a + b. Fix only this bug and run cargo test to verify all tests pass.",
            "Rust",
            3,
            "language_guard: Rust bugfix",
        )
        .with_scaffold(
            "Cargo.toml",
            "[package]\nname = \"bugfix\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
        )
        .with_scaffold(
            "src/lib.rs",
            r#"pub fn add(a: i32, b: i32) -> i32 {
    a - b  // BUG: should be a + b
}

pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_add() {
        assert_eq!(add(2, 3), 5);
        assert_eq!(add(-1, 1), 0);
        assert_eq!(add(0, 0), 0);
    }
    #[test]
    fn test_multiply() {
        assert_eq!(multiply(3, 4), 12);
        assert_eq!(multiply(0, 5), 0);
    }
}
"#,
        ),

        BenchCase::new(
            "BugFix Go: wrong return",
            "Fix the bug in calculator.go: the Multiply function returns a + b instead of a * b. Fix only this function. The existing tests in calculator_test.go must pass.",
            "Go",
            3,
            "language_guard: Go bugfix",
        )
        .with_scaffold("go.mod", "module calculator\n\ngo 1.21\n")
        .with_scaffold(
            "calculator.go",
            r#"package calculator

func Add(a, b int) int {
    return a + b
}

func Multiply(a, b int) int {
    return a + b  // BUG: should be a * b
}
"#,
        )
        .with_scaffold(
            "calculator_test.go",
            r#"package calculator

import "testing"

func TestAdd(t *testing.T) {
    if Add(2, 3) != 5 { t.Error("Add failed") }
}

func TestMultiply(t *testing.T) {
    if Multiply(3, 4) != 12 { t.Errorf("Multiply(3,4): expected 12") }
    if Multiply(0, 5) != 0 { t.Errorf("Multiply(0,5): expected 0") }
    if Multiply(-2, 3) != -6 { t.Errorf("Multiply(-2,3): expected -6") }
}
"#,
        ),

        BenchCase::new(
            "BugFix Python: off-by-one",
            "Fix the bug in stats.py: the average function divides by len(nums)-1 instead of len(nums). Fix this bug. All tests in test_stats.py must pass.",
            "Python",
            3,
            "language_guard: Python bugfix",
        )
        .with_scaffold(
            "stats.py",
            r#"def average(nums):
    if not nums:
        raise ValueError("empty list")
    return sum(nums) / (len(nums) - 1)  # BUG: should divide by len(nums)

def maximum(nums):
    if not nums:
        raise ValueError("empty list")
    return max(nums)

def minimum(nums):
    if not nums:
        raise ValueError("empty list")
    return min(nums)
"#,
        )
        .with_tests(
            "test_stats.py",
            r#"import pytest
from stats import average, maximum, minimum

def test_average():
    assert average([1, 2, 3]) == 2.0
    assert average([10, 20]) == 15.0
    assert average([5]) == 5.0

def test_average_empty():
    with pytest.raises(ValueError):
        average([])

def test_maximum():
    assert maximum([1, 5, 3]) == 5
    assert maximum([-1, -5, -3]) == -1

def test_minimum():
    assert minimum([1, 5, 3]) == 1
    assert minimum([-1, -5, -3]) == -5
"#,
        ),

        // ────────────────────────────────────────────────────────────
        // TIER 4: Real-World Patterns
        // هدف: السيناريوهات الحقيقية الأكثر طلباً
        // ────────────────────────────────────────────────────────────
        BenchCase::new(
            "Real: Python CLI wordcount",
            "Build a Python module 'wordcount.py' with two functions: count_words(text: str) -> dict that counts word frequencies (case-insensitive), and top_words(counts: dict, n: int) -> list of (word, count) tuples sorted by frequency descending.\n\nTests: python -m pytest test_wordcount.py -v",
            "Python",
            4,
            "real: CLI utility",
        )
        .with_tests(
            "test_wordcount.py",
            r#"from wordcount import count_words, top_words

def test_count_words_basic():
    result = count_words("hello world hello")
    assert result["hello"] == 2
    assert result["world"] == 1

def test_count_words_case_insensitive():
    result = count_words("Hello HELLO hello")
    assert result["hello"] == 3

def test_count_words_empty():
    result = count_words("")
    assert result == {}

def test_top_words():
    counts = {"a": 5, "b": 3, "c": 8, "d": 1}
    top = top_words(counts, n=2)
    assert len(top) == 2
    assert top[0] == ("c", 8)
    assert top[1] == ("a", 5)

def test_top_words_less_than_n():
    counts = {"x": 1}
    top = top_words(counts, n=5)
    assert len(top) == 1
"#,
        ),

        BenchCase::new(
            "Real: Go grep-lite",
            "Build a Go file 'grep.go' in package main with an exported function MatchLines(lines []string, pattern string) []string that returns lines matching the regex pattern. Import regexp. Also write a main() that reads from os.Stdin line by line and prints matches for os.Args[1] pattern.\n\nTests: go test -v",
            "Go",
            4,
            "real: CLI with regex",
        )
        .with_tests(
            "grep_test.go",
            r#"package main

import "testing"

func TestMatchLines(t *testing.T) {
    input := []string{"hello world", "foo bar", "hello again", "test"}
    result := MatchLines(input, "hello")
    if len(result) != 2 {
        t.Errorf("Expected 2 matches, got %d", len(result))
    }
}

func TestMatchLinesEmpty(t *testing.T) {
    result := MatchLines([]string{}, "test")
    if len(result) != 0 {
        t.Errorf("Expected 0 matches on empty input, got %d", len(result))
    }
}

func TestMatchLinesNoMatch(t *testing.T) {
    input := []string{"apple", "banana", "cherry"}
    result := MatchLines(input, "^z")
    if len(result) != 0 {
        t.Errorf("Expected 0 matches, got %d", len(result))
    }
}

func TestMatchLinesRegex(t *testing.T) {
    input := []string{"error: file not found", "info: started", "error: timeout"}
    result := MatchLines(input, "^error")
    if len(result) != 2 {
        t.Errorf("Expected 2 error lines, got %d", len(result))
    }
}
"#,
        )
        .with_scaffold("go.mod", "module grep-lite\n\ngo 1.21\n"),

        BenchCase::new(
            "Real: TypeScript validator",
            "Create a TypeScript file 'validator.ts' exporting three functions: isEmail(s: string): boolean, isUrl(s: string): boolean, isStrongPassword(s: string, minLen?: number): boolean (default minLen=8, requires uppercase + lowercase + digit).\n\nTests: npx jest validator.test.ts",
            "TypeScript",
            4,
            "real: TS library",
        )
        .with_tests(
            "validator.test.ts",
            r#"import { isEmail, isUrl, isStrongPassword } from './validator';

describe('isEmail', () => {
    test('valid emails', () => {
        expect(isEmail('user@example.com')).toBe(true);
        expect(isEmail('a@b.co')).toBe(true);
    });
    test('invalid emails', () => {
        expect(isEmail('not-an-email')).toBe(false);
        expect(isEmail('@missing.com')).toBe(false);
        expect(isEmail('missing@')).toBe(false);
    });
});

describe('isUrl', () => {
    test('valid urls', () => {
        expect(isUrl('https://example.com')).toBe(true);
        expect(isUrl('http://localhost:3000')).toBe(true);
    });
    test('invalid urls', () => {
        expect(isUrl('not a url')).toBe(false);
        expect(isUrl('ftp://old.com')).toBe(false);
    });
});

describe('isStrongPassword', () => {
    test('strong passwords', () => {
        expect(isStrongPassword('Abcde123', 8)).toBe(true);
        expect(isStrongPassword('MyPass9', 6)).toBe(true);
    });
    test('weak passwords', () => {
        expect(isStrongPassword('short', 8)).toBe(false);
        expect(isStrongPassword('alllowercase1', 8)).toBe(false);
        expect(isStrongPassword('ALLUPPERCASE1', 8)).toBe(false);
        expect(isStrongPassword('NoDigitsHere', 8)).toBe(false);
    });
});
"#,
        ),

        BenchCase::new(
            "Real: Rust calculator library",
            "Create a complete Rust library in src/lib.rs for a stack-based calculator. Implement: pub struct Calculator with a stack (Vec<f64>), pub fn push(&mut self, val: f64), pub fn pop(&mut self) -> Result<f64, String>, pub fn add(&mut self) -> Result<f64, String>, pub fn multiply(&mut self) -> Result<f64, String>. Include full #[cfg(test)] module.\n\nTests: cargo test",
            "Rust",
            4,
            "real: Rust library",
        )
        .with_scaffold(
            "Cargo.toml",
            "[package]\nname = \"stack-calc\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
        ),
    ]
}

// ═══════════════════════════════════════════════════════════════════════════
// طباعة الخطة
// ═══════════════════════════════════════════════════════════════════════════

fn print_test_plan(cases: &[BenchCase]) {
    println!("📋 Test Plan ({} cases):\n", cases.len());

    let mut by_tier: std::collections::BTreeMap<u8, Vec<&BenchCase>> =
        std::collections::BTreeMap::new();
    for case in cases {
        by_tier.entry(case.tier).or_default().push(case);
    }

    let tier_names = [
        (1u8, "Compile-First Pipeline"),
        (2u8, "quick_fix (auto, no LLM)"),
        (3u8, "Language Guard + BugFix"),
        (4u8, "Real-World Patterns"),
    ];

    for (tier, name) in &tier_names {
        if let Some(tier_cases) = by_tier.get(tier) {
            println!("  {} Tier {} — {} ({} cases)",
                "●".yellow(), tier, name.bold(), tier_cases.len());
            for c in tier_cases {
                println!("      {} [{:12}] {}",
                    "→".dimmed(),
                    c.lang.blue(),
                    c.tests_feature.magenta());
            }
        }
    }
    println!();
}

// ═══════════════════════════════════════════════════════════════════════════
// طباعة النتائج
// ═══════════════════════════════════════════════════════════════════════════

fn print_results(
    passed: usize,
    total: usize,
    total_repairs: usize,
    elapsed: Duration,
    feature_stats: &std::collections::HashMap<&str, (usize, usize)>,
    tier: Option<u8>,
) {
    let pct = if total > 0 { (passed as f64 / total as f64) * 100.0 } else { 0.0 };
    let avg_r = if passed > 0 { total_repairs as f64 / passed as f64 } else { 0.0 };

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║   SEL Agent v7.3 — Benchmark Results                        ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Tier    : {}",
        tier.map_or("ALL".to_string(), |t| format!("Tier {}", t)));
    println!("║  Time    : {}m {}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60);
    println!("║  Result  : {}/{} ({:.1}%)", passed, total, pct);
    println!("║  AvgFix  : {:.1} repairs/success", avg_r);
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Feature Breakdown                                          ║");
    println!("╠══════════════════════════════════════════════════════════════╣");

    let mut sorted: Vec<_> = feature_stats.iter().collect();
    sorted.sort_by_key(|(k, _)| *k);

    for (feature, (p, t)) in &sorted {
        let fpct = if *t > 0 { (*p as f64 / *t as f64) * 100.0 } else { 0.0 };
        let filled = (fpct / 10.0) as usize;
        let bar_raw = format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled));
        let bar = if fpct >= 80.0 { bar_raw.green().to_string() }
                  else if fpct >= 50.0 { bar_raw.yellow().to_string() }
                  else { bar_raw.red().to_string() };
        println!("║  {:<38} {} {}/{}", feature, bar, p, t);
    }

    println!("╠══════════════════════════════════════════════════════════════╣");

    let verdict = if pct >= 90.0 {
        format!("  {} STABLE — ready for v7.4 planning", "✅".green())
    } else if pct >= 70.0 {
        format!("  {} FUNCTIONAL — investigate failures before proceeding", "⚠️".yellow())
    } else {
        format!("  {} UNSTABLE — fix issues before any new feature", "❌".red())
    };

    println!("║  {}  ║", verdict);
    println!("╚══════════════════════════════════════════════════════════════╝\n");
}

```

## File: chunker.rs

```rust
// ============================================================
// src/chunker.rs  —  SEL Agent v5.8
// Context Chunking: يحل مشكلة 413 Payload Too Large
// ============================================================
//
// المنطق الأساسي:
//   1. استخراج file:line من مخرجات الاختبارات الفاشلة
//   2. إذا كان الملف > MAX_FILE_LINES → اقرأ فقط ±CHUNK_RADIUS سطر
//   3. أضف أرقام الأسطر في المحتوى المُرسل للـ LLM
//   4. أضف context_hint في الـ prompt يخبر اللغوي أنه يرى جزءاً من الملف
// ============================================================

use std::fs;
use std::path::Path;

// ─── ثوابت ───────────────────────────────────────────────────
pub const MAX_FILE_LINES: usize = 400; // فوق هذا → نستخدم chunking
pub const CHUNK_RADIUS: usize = 60; // ±60 سطر حول الخطأ
pub const CHARS_PER_TOKEN: usize = 4; // تقدير: 1 token ≈ 4 حرف
pub const MAX_TOKENS_PER_FILE: usize = 3_000; // ~12K حرف كحد أقصى لملف واحد

// ─── البنى ───────────────────────────────────────────────────

/// موقع خطأ مستخرج من مخرجات الاختبار
#[derive(Debug, Clone)]
pub struct ErrorLocation {
    /// المسار النسبي أو المطلق للملف
    pub file: String,
    /// رقم السطر (1-indexed)
    pub line: usize,
}

/// نتيجة قراءة chunk من ملف كبير
#[derive(Debug)]
pub struct FileChunk {
    /// المحتوى مع أرقام الأسطر مضافة
    pub content: String,
    /// رقم السطر الأول في الـ chunk
    pub start_line: usize,
    /// رقم السطر الأخير في الـ chunk
    pub end_line: usize,
    /// إجمالي أسطر الملف الأصلي
    pub total_lines: usize,
}

// ─── استخراج مواقع الأخطاء ────────────────────────────────────

/// يستخرج مواقع الأخطاء من مخرجات الاختبارات
///
/// يدعم صياغات:
/// - Rust:   `src/main.rs:42:10` أو `--> src/main.rs:42`
/// - Python: `File "src/main.py", line 42`
/// - Go:     `src/main.go:42:`
/// - Node:   `src/main.js:42`
pub fn extract_error_locations(test_output: &str) -> Vec<ErrorLocation> {
    let mut locations: Vec<ErrorLocation> = Vec::new();

    for line in test_output.lines() {
        if let Some(loc) = parse_rust_location(line) {
            if !is_duplicate(&locations, &loc) {
                locations.push(loc);
            }
            continue;
        }

        if let Some(loc) = parse_python_location(line) {
            if !is_duplicate(&locations, &loc) {
                locations.push(loc);
            }
            continue;
        }

        if let Some(loc) = parse_generic_location(line) {
            if !is_duplicate(&locations, &loc) {
                locations.push(loc);
            }
        }
    }

    locations.truncate(5);
    locations
}

fn parse_rust_location(line: &str) -> Option<ErrorLocation> {
    let line = line.trim();

    let search_str = if line.starts_with("-->") {
        line.trim_start_matches("-->").trim()
    } else if line.contains(" --> ") {
        line.split(" --> ").nth(1)?
    } else {
        line
    };

    parse_file_line_col(search_str, &[".rs"])
}

fn parse_python_location(line: &str) -> Option<ErrorLocation> {
    let line = line.trim();

    if !line.contains("File ") {
        return None;
    }

    let file_pos = line.find("File ")?;
    let after_file = &line[file_pos + 5..];
    let (path, rest) = if after_file.starts_with('"') {
        let end = after_file[1..].find('"')? + 1;
        (&after_file[1..end], &after_file[end + 1..])
    } else if after_file.starts_with('\'') {
        let end = after_file[1..].find('\'')? + 1;
        (&after_file[1..end], &after_file[end + 1..])
    } else {
        return None;
    };

    if !path.ends_with(".py") {
        return None;
    }

    let line_part = rest.trim().strip_prefix(',')?;
    let line_part = line_part.trim().strip_prefix("line")?;
    let line_num: usize = line_part
        .trim()
        .split_whitespace()
        .next()?
        .trim_end_matches(',')
        .parse()
        .ok()?;

    Some(ErrorLocation {
        file: path.to_string(),
        line: line_num,
    })
}

fn parse_generic_location(line: &str) -> Option<ErrorLocation> {
    parse_file_line_col(line.trim(), &[".go", ".js", ".ts", ".java", ".c"])
}

fn parse_file_line_col(text: &str, extensions: &[&str]) -> Option<ErrorLocation> {
    let parts: Vec<&str> = text.splitn(4, ':').collect();
    if parts.len() < 2 {
        return None;
    }

    for i in 0..parts.len().saturating_sub(1) {
        let file_part = parts[..=i].join(":");
        let file_part = file_part.trim();

        let has_valid_ext = if extensions.contains(&".rs") {
            file_part.ends_with(".rs")
        } else {
            extensions.iter().any(|ext| file_part.ends_with(ext))
        };

        if !has_valid_ext {
            continue;
        }

        if let Some(line_str) = parts.get(i + 1) {
            let line_str = line_str.split_whitespace().next().unwrap_or("");
            let line_str = line_str.trim_end_matches(':');
            if let Ok(line_num) = line_str.parse::<usize>() {
                if line_num > 0 && line_num < 100_000 {
                    return Some(ErrorLocation {
                        file: file_part.to_string(),
                        line: line_num,
                    });
                }
            }
        }
    }
    None
}

fn is_duplicate(locations: &[ErrorLocation], new: &ErrorLocation) -> bool {
    locations
        .iter()
        .any(|l| l.file == new.file && (l.line as i64 - new.line as i64).abs() < 10)
}

// ─── قراءة Chunk من ملف كبير ─────────────────────────────────

/// يقرأ chunk من ملف كبير حول سطر محدد
/// يُضيف أرقام الأسطر في البداية لمساعدة اللغوي على الإشارة بدقة
pub fn read_file_chunk(path: &Path, center_line: usize, radius: usize) -> Option<FileChunk> {
    let content = fs::read_to_string(path).ok()?;
    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();

    if total_lines == 0 {
        return None;
    }

    let center_idx = center_line.saturating_sub(1).min(total_lines - 1);
    let start_idx = center_idx.saturating_sub(radius);
    let end_idx = (center_idx + radius).min(total_lines - 1);

    let mut chunk_lines = Vec::new();
    for (i, line) in all_lines[start_idx..=end_idx].iter().enumerate() {
        let line_num = start_idx + i + 1;
        chunk_lines.push(format!("{:5}: {}", line_num, line));
    }

    Some(FileChunk {
        content: chunk_lines.join("\n"),
        start_line: start_idx + 1,
        end_line: end_idx + 1,
        total_lines,
    })
}

/// يقرر هل يُرسل الملف كاملاً أم chunk
pub fn get_file_content_smart(
    path: &Path,
    error_locations: &[ErrorLocation],
) -> Result<SmartContent, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("خطأ في قراءة {:?}: {}", path, e))?;

    let line_count = content.lines().count();
    let token_estimate = content.len() / CHARS_PER_TOKEN;

    if line_count <= MAX_FILE_LINES && token_estimate <= MAX_TOKENS_PER_FILE {
        return Ok(SmartContent::FullFile(content));
    }

    let path_str = path.to_string_lossy();
    let center_line = error_locations
        .iter()
        .find(|loc| path_str.contains(&loc.file) || loc.file.contains(path_str.as_ref()))
        .map(|loc| loc.line)
        .unwrap_or(1);

    match read_file_chunk(path, center_line, CHUNK_RADIUS) {
        Some(chunk) => Ok(SmartContent::Chunk(chunk)),
        None => Ok(SmartContent::FullFile(content)),
    }
}

// ─── SmartContent ─────────────────────────────────────────────

pub enum SmartContent {
    FullFile(String),
    Chunk(FileChunk),
}

impl SmartContent {
    pub fn content_for_prompt(&self, file_name: &str) -> String {
        match self {
            SmartContent::FullFile(content) => content.clone(),
            SmartContent::Chunk(chunk) => format!(
                "[[ CHUNK: {} — أسطر {}-{} من {} ]]\n{}",
                file_name, chunk.start_line, chunk.end_line, chunk.total_lines, chunk.content
            ),
        }
    }

    pub fn is_chunk(&self) -> bool {
        matches!(self, SmartContent::Chunk(_))
    }

    pub fn context_hint(&self, file_name: &str) -> Option<String> {
        match self {
            SmartContent::FullFile(_) => None,
            SmartContent::Chunk(chunk) => Some(format!(
                "⚠️ CHUNKED FILE: '{}' يحتوي على {} سطر. تظهر لك فقط الأسطر {}-{}. \
                 عند كتابة الـ patch، استخدم أرقام الأسطر الظاهرة. \
                 لا تفترض بداية الملف = سطر 1.",
                file_name, chunk.total_lines, chunk.start_line, chunk.end_line,
            )),
        }
    }
}

// ─── تقدير الـ tokens ─────────────────────────────────────────

pub fn estimate_tokens(text: &str) -> usize {
    text.len() / CHARS_PER_TOKEN
}

pub fn estimate_context_tokens(files: &[(String, String)]) -> usize {
    files
        .iter()
        .map(|(_, content)| estimate_tokens(content))
        .sum()
}

// ─── اختبارات الوحدة ──────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust_location_arrow() {
        let output = "error[E0308]: mismatched types\n  --> src/main.rs:42:10\n   |";
        let locs = extract_error_locations(output);
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].file, "src/main.rs");
        assert_eq!(locs[0].line, 42);
    }

    #[test]
    fn test_extract_python_location() {
        let output = "Traceback (most recent call last):\n  File \"src/core.py\", line 1217, in make_context\nAssertionError";
        let locs = extract_error_locations(output);
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].file, "src/core.py");
        assert_eq!(locs[0].line, 1217);
    }

    #[test]
    fn test_extract_go_location() {
        let output = "FAIL\nmain_test.go:34: got nil, want error";
        let locs = extract_error_locations(output);
        assert!(locs
            .iter()
            .any(|l| l.file.contains("main_test.go") && l.line == 34));
    }

    #[test]
    fn test_chunk_radius() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        for i in 1..=500 {
            writeln!(tmp, "line {}", i).unwrap();
        }
        let chunk = read_file_chunk(tmp.path(), 250, 60).unwrap();
        assert_eq!(chunk.start_line, 190);
        assert_eq!(chunk.end_line, 310);
        assert!(chunk.content.contains("  190:"));
        assert!(chunk.content.contains("  310:"));
    }

    #[test]
    fn test_chunk_at_start_of_file() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        for i in 1..=500 {
            writeln!(tmp, "line {}", i).unwrap();
        }
        let chunk = read_file_chunk(tmp.path(), 10, 60).unwrap();
        assert_eq!(chunk.start_line, 1);
        assert_eq!(chunk.end_line, 70);
    }

    #[test]
    fn test_estimate_tokens() {
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }

    #[test]
    fn test_no_duplicates_close_lines() {
        let output = "  --> src/main.rs:42:10\n  --> src/main.rs:43:5";
        let locs = extract_error_locations(output);
        assert_eq!(locs.len(), 1);
    }

    #[test]
    fn test_smart_content_full_file_small() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        for i in 1..=100 {
            writeln!(tmp, "fn line_{}() {{}}", i).unwrap();
        }
        let result = get_file_content_smart(tmp.path(), &[]).unwrap();
        assert!(!result.is_chunk());
    }

    #[test]
    fn test_smart_content_chunk_large() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        for i in 1..=600 {
            writeln!(tmp, "fn line_{}() {{ /* code */ }}", i).unwrap();
        }
        let loc = ErrorLocation {
            file: "test".into(),
            line: 300,
        };
        let result = get_file_content_smart(tmp.path(), &[loc]).unwrap();
        assert!(result.is_chunk());
    }

    #[test]
    fn test_context_hint_format() {
        let chunk = FileChunk {
            content: "1210: fn foo() {}".into(),
            start_line: 1210,
            end_line: 1270,
            total_lines: 1800,
        };
        let smart = SmartContent::Chunk(chunk);
        let hint = smart.context_hint("src/core.py").unwrap();
        assert!(hint.contains("1800"));
        assert!(hint.contains("1210-1270"));
    }
}

```

## File: constitution.rs

```rust
// src/constitution.rs — الدستور الحاكم لسلوك النموذج
// أي تحسين أو تقييد للسلوك يُضاف هنا، ويُطبق تلقائياً على كل الاستدعاءات.
// هذا الملف هو الطريقة الجذرية (بدون ضمادات) لإدارة قواعد الـ Prompt.

pub const CONSTITUTION: &str = r#"
<SYSTEM_CONSTITUTION>
These rules are absolute. Violating them results in immediate task failure.

1. LANGUAGE LOCK:
   - Identify the primary language from existing files in the workspace.
   - You MUST ONLY write, modify, and test files matching that language.
   - NEVER create Python (.py) files to solve Go/Rust/TypeScript errors.
   - NEVER create Go (.go) files to solve Python errors.
   - Cross-language escapes are strictly FORBIDDEN.

2. PROJECT INTEGRITY:
   - NEVER run initialization commands (`go mod init`, `cargo new`, `npm init`, `python3 -m venv`) 
     if the project infrastructure (go.mod, Cargo.toml, package.json, venv) already exists.
   - Work WITHIN the existing structure. Do not attempt to rebuild the project.

3. CODE PRESERVATION:
   - When patching a file, you MUST preserve all existing functions, structs, classes, and imports 
     that are not the direct cause of the error.
   - Never overwrite a file with a smaller version that loses previous functionality.

4. UNICODE BAN:
   - NEVER use Unicode quotes (no “ ” or ‘ ’). Always use ASCII only: " and '
   - Unicode quotes WILL cause compile errors.

5. TEST INTEGRITY (NO CHEATING):
   - NEVER modify test files to bypass errors, weaken assertions, or adapt tests to fit broken code.
   - If tests are failing, you MUST fix the logic in the source code, NOT the tests.
   - The test requirements define the absolute ground truth.

6. ALGORITHM CORRECTNESS (COMMON HALLUCINATIONS — AVOID):
   a) EMAIL VALIDATION:
      - TLD can be 1+ chars: a@b.c is VALID. Use pattern: [a-zA-Z]{1,} NOT {2,}
      - Correct regex: r'^[^@\s]+@[^@\s]+\.[a-zA-Z]{1,}$'
   b) SLUGIFY:
      - Replace ALL non-alphanumeric characters with hyphens, then lowercase.
      - "Hello World!" → "hello-world" not "helloworld"
      - Use: re.sub(r'[^a-z0-9]+', '-', text.lower()).strip('-')
   c) TRUNCATE:
      - truncate(text, length, suffix): if len(text) <= length: return text
      - Trim text to (length - len(suffix)) chars, then append suffix.
      - "Test", length=3, suffix="*" → "T*" (2 chars from text + 1 from suffix = 3)
   d) MASK SENSITIVE:
      - mask_sensitive(text, show_start, show_end): show first N and last M chars, mask middle.
      - If show_start=0 and show_end=0: return all stars "*" * len(text)
</SYSTEM_CONSTITUTION>
"#;

```

## File: constraint_engine.rs

```rust
//! SEL Agent v6.3 - Constraint Engine
//!
//! مسؤولياته:
//! 1. Environment Lock: منع pytest في Node، منع npm في Python
//! 2. Config Deduplication: منع كتابة jest.config.js إذا كان package.json يحتوي jest
//! 3. Dependency Normalization: استبدال versions خاطئة بـ pinned stacks
//! 4. Fatal Constraints: إيقاف الخطط الفاشلة تمامًا
//!
//! الترتيب مهم:
//! 1️⃣ environment::enforce → 2️⃣ config::deduplicate → 3️⃣ dependencies::normalize

use crate::protocol::Cmd;
use crate::scanner::has_ts_files;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ============================================================================
// أنواع البيانات الأساسية
// ============================================================================

/// بيئة المشروع - تُحسب مرة واحدة من Scanner
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectEnv {
    Node { has_typescript: bool },
    Python,
    Rust,
    Go,
    Unknown,
}

impl ProjectEnv {
    /// كشف البيئة من workspace
    pub fn detect(workspace: &Path) -> Self {
        if workspace.join("Cargo.toml").exists() {
            return ProjectEnv::Rust;
        }
        if workspace.join("go.mod").exists() {
            return ProjectEnv::Go;
        }
        if workspace.join("requirements.txt").exists()
            || workspace.join("pyproject.toml").exists()
        {
            return ProjectEnv::Python;
        }
        if workspace.join("package.json").exists() {
            let has_ts = has_ts_files(workspace);
            return ProjectEnv::Node { has_typescript: has_ts };
        }
        ProjectEnv::Unknown
    }
}

/// حالة المشروع على disk - ما هو موجود فعلاً
#[derive(Debug, Clone, Default)]
pub struct ProjectState {
    pub files: HashSet<String>,
    pub has_jest_config: bool,
    pub has_tsconfig: bool,
    pub has_package_json: bool,
    pub jest_in_pkg_json: bool,
}

impl ProjectState {
    /// مسح workspace لمعرفة الملفات الموجودة
    pub fn scan(workspace: &Path) -> Self {
        let mut files = HashSet::new();
        let has_jest_config;
        let has_tsconfig;
        let has_package_json;
        let jest_in_pkg_json;

        // فحص jest.config.js
        if workspace.join("jest.config.js").exists() {
            files.insert("jest.config.js".to_string());
            has_jest_config = true;
        } else if workspace.join("jest.config.ts").exists() {
            files.insert("jest.config.ts".to_string());
            has_jest_config = true;
        } else {
            has_jest_config = false;
        }

        // فحص tsconfig.json
        if workspace.join("tsconfig.json").exists() {
            files.insert("tsconfig.json".to_string());
            has_tsconfig = true;
        } else {
            has_tsconfig = false;
        }

        // فحص package.json ووجود jest field
        let pkg_path = workspace.join("package.json");
        if pkg_path.exists() {
            files.insert("package.json".to_string());
            has_package_json = true;
            jest_in_pkg_json = Self::check_jest_in_package_json(&pkg_path);
        } else {
            has_package_json = false;
            jest_in_pkg_json = false;
        }

        Self {
            files,
            has_jest_config,
            has_tsconfig,
            has_package_json,
            jest_in_pkg_json,
        }
    }

    fn check_jest_in_package_json(path: &Path) -> bool {
        let content = std::fs::read_to_string(path).ok()?;
        if let Ok(json) = serde_json::from_str::<Value>(&content) {
            if json.get("jest").is_some() {
                return true;
            }
        }
        false
    }
}

/// نتيجة تطبيق القيود
#[derive(Debug, Clone)]
pub enum ConstraintResult {
    /// نجاح - الأوامر المعدلة (أو الأصلية)
    Ok(Vec<Cmd>),
    /// فشل قاتل - يجب إعادة التخطيط من الصفر
    Fatal(String),
}

// ============================================================================
// Pinned Stacks - النسخ المضمونة المتوافقة
// ============================================================================

/// Pinned Stack لـ TypeScript + Jest
const TS_JEST_STACK: &str = "typescript@5.3.3 ts-jest@29.1.1 jest@29.7.0 @types/jest@29.5.11";

/// قالب package.json الصحيح لـ TypeScript
const CORRECT_PACKAGE_JSON_TS: &str = r#"{
  "scripts": {
    "test": "jest",
    "build": "tsc"
  },
  "devDependencies": {
    "typescript": "5.3.3",
    "ts-jest": "29.1.1",
    "jest": "29.7.0",
    "@types/jest": "29.5.11"
  },
  "jest": {
    "preset": "ts-jest",
    "testEnvironment": "node"
  }
}"#;

// ============================================================================
// القاعدة 1: Environment Lock
// ============================================================================

/// منع أوامر خارج بيئة المشروع
fn enforce_environment(plan: Vec<Cmd>, env: &ProjectEnv) -> Vec<Cmd> {
    let mut filtered = Vec::new();

    for cmd in plan {
        match cmd {
            Cmd::Run { ref command } => {
                let cmd_lower = command.to_lowercase();
                let is_allowed = match env {
                    ProjectEnv::Node { .. } => {
                        // منع أوامر Python في Node
                        !(cmd_lower.contains("pytest")
                            || cmd_lower.contains("venv")
                            || cmd_lower.contains("pip")
                            || cmd_lower.contains("python"))
                    }
                    ProjectEnv::Python => {
                        // منع أوامر Node في Python
                        !(cmd_lower.contains("npm")
                            || cmd_lower.contains("npx")
                            || cmd_lower.contains("node"))
                    }
                    ProjectEnv::Rust => {
                        // منع npm و pip في Rust
                        !(cmd_lower.contains("npm")
                            || cmd_lower.contains("pip")
                            || cmd_lower.contains("pytest"))
                    }
                    ProjectEnv::Go => {
                        // منع npm و pip في Go
                        !(cmd_lower.contains("npm") || cmd_lower.contains("pip"))
                    }
                    ProjectEnv::Unknown => true,
                };

                if is_allowed {
                    filtered.push(cmd);
                } else {
                    eprintln!("🔒 Constraint: blocked '{}' (wrong environment)", command);
                }
            }
            _ => filtered.push(cmd),
        }
    }

    filtered
}

// ============================================================================
// القاعدة 2: Config Deduplication - منع jest.config.js
// ============================================================================

/// معالجة WriteFile لـ package.json و jest.config.js
fn intercept_write_file(
    path: &str,
    content: &str,
    state: &ProjectState,
    env: &ProjectEnv,
) -> Option<Cmd> {
    let path_lower = path.to_lowercase();

    // حالة 1: كتابة jest.config.js
    if path_lower.ends_with("jest.config.js") || path_lower.ends_with("jest.config.ts") {
        // التحقق: هل package.json موجود وفيه jest field؟
        if state.has_package_json && state.jest_in_pkg_json {
            eprintln!(
                "🔒 Constraint: blocked write '{}' (jest already in package.json)",
                path
            );
            return None; // تجاهل الكتابة تمامًا
        }

        // إذا لم يكن هناك package.json أو ليس فيه jest field، نحتاج إلى تعديل package.json
        if state.has_package_json {
            eprintln!("🔒 Constraint: converting jest.config.js → merge into package.json");
            // سنقوم بإضافة jest config إلى package.json بدلاً من إنشاء الملف
            // هذا يتم في normalize_package_json
            return None; // نمنع الكتابة، وnormalize_package_json سيتولى الباقي
        }
    }

    // حالة 2: كتابة package.json - نحتاج إلى تطبيع المحتوى
    if path_lower.ends_with("package.json") {
        return Some(Cmd::WriteFile {
            path: path.to_string(),
            content: normalize_package_json(content, state, env).to_string(),
        });
    }

    None
}

/// تطبيع package.json: إضافة pinned stack، إزالة jest field إذا كان هناك jest.config.js
fn normalize_package_json(content: &str, state: &ProjectState, env: &ProjectEnv) -> String {
    let mut json: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("⚠️ Constraint: invalid package.json, using template");
            return CORRECT_PACKAGE_JSON_TS.to_string();
        }
    };

    // فقط لـ TypeScript projects
    let is_ts_project = match env {
        ProjectEnv::Node { has_typescript } => *has_typescript,
        _ => false,
    };

    if !is_ts_project {
        return content.to_string();
    }

    // 1. تطبيق pinned stack على devDependencies
    let dev_deps = json
        .get_mut("devDependencies")
        .and_then(|d| d.as_object_mut());

    if let Some(deps) = dev_deps {
        // استبدال typescript بالنسخة المثبتة
        deps.insert("typescript".to_string(), Value::String("5.3.3".to_string()));
        deps.insert("ts-jest".to_string(), Value::String("29.1.1".to_string()));
        deps.insert("jest".to_string(), Value::String("29.7.0".to_string()));
        deps.insert(
            "@types/jest".to_string(),
            Value::String("29.5.11".to_string()),
        );
    } else {
        // إذا لم يكن devDependencies موجودًا، أضفه
        let mut deps = serde_json::Map::new();
        deps.insert("typescript".to_string(), Value::String("5.3.3".to_string()));
        deps.insert("ts-jest".to_string(), Value::String("29.1.1".to_string()));
        deps.insert("jest".to_string(), Value::String("29.7.0".to_string()));
        deps.insert(
            "@types/jest".to_string(),
            Value::String("29.5.11".to_string()),
        );
        json["devDependencies"] = Value::Object(deps);
    }

    // 2. تطبيق scripts الصحيحة
    let scripts = json.get_mut("scripts").and_then(|s| s.as_object_mut());
    if let Some(scripts) = scripts {
        // فقط إذا لم يكن هناك script موجود أو كان خاطئًا
        if !scripts.contains_key("test") {
            scripts.insert("test".to_string(), Value::String("jest".to_string()));
        }
        if !scripts.contains_key("build") {
            scripts.insert("build".to_string(), Value::String("tsc".to_string()));
        }
    } else {
        let mut scripts = serde_json::Map::new();
        scripts.insert("test".to_string(), Value::String("jest".to_string()));
        scripts.insert("build".to_string(), Value::String("tsc".to_string()));
        json["scripts"] = Value::Object(scripts);
    }

    // 3. إضافة jest config داخل package.json (إذا لم يكن هناك jest.config.js)
    if !state.has_jest_config {
        let jest_config = json.get_mut("jest").and_then(|j| j.as_object_mut());
        if jest_config.is_none() {
            let mut jest = serde_json::Map::new();
            jest.insert("preset".to_string(), Value::String("ts-jest".to_string()));
            jest.insert(
                "testEnvironment".to_string(),
                Value::String("node".to_string()),
            );
            json["jest"] = Value::Object(jest);
        }
    } else {
        // إذا كان هناك jest.config.js، نزيل jest field من package.json
        if json.get("jest").is_some() {
            eprintln!("🔒 Constraint: removing 'jest' field from package.json (jest.config.js exists)");
            json.as_object_mut().and_then(|obj| obj.remove("jest"));
        }
    }

    // 4. التأكد من عدم وجود 'type': 'module' (يسبب مشاكل مع jest)
    if let Some(r#type) = json.get("type") {
        if r#type == "module" {
            eprintln!("🔒 Constraint: removing 'type': 'module' from package.json");
            json.as_object_mut().and_then(|obj| obj.remove("type"));
        }
    }

    serde_json::to_string_pretty(&json).unwrap_or_else(|_| content.to_string())
}

// ============================================================================
// القاعدة 3: Dependency Normalization
// ============================================================================

/// تطبيع أوامر npm install لاستخدام pinned stacks
fn normalize_dependencies(plan: Vec<Cmd>, env: &ProjectEnv) -> Vec<Cmd> {
    let is_ts_project = match env {
        ProjectEnv::Node { has_typescript } => *has_typescript,
        _ => false,
    };

    if !is_ts_project {
        return plan;
    }

    let mut normalized = Vec::new();

    for cmd in plan {
        match cmd {
            Cmd::Run { command } => {
                let cmd_lower = command.to_lowercase();

                // npm install typescript ts-jest ...
                if cmd_lower.contains("npm install")
                    && (cmd_lower.contains("typescript") || cmd_lower.contains("ts-jest"))
                {
                    eprintln!("🔒 Constraint: normalizing npm install → using pinned stack");
                    normalized.push(Cmd::Run {
                        command: format!("npm install {}", TS_JEST_STACK),
                    });
                } else {
                    normalized.push(Cmd::Run { command });
                }
            }
            _ => normalized.push(cmd),
        }
    }

    normalized
}

// ============================================================================
// القاعدة 4: Fatal Constraints
// ============================================================================

/// فحص fatal errors - خطط فاشلة بالكامل
fn check_fatal(plan: &[Cmd], env: &ProjectEnv) -> Option<String> {
    let mut has_npm = false;
    let mut has_pip = false;

    for cmd in plan {
        if let Cmd::Run { command } = cmd {
            let cmd_lower = command.to_lowercase();
            if cmd_lower.contains("npm") || cmd_lower.contains("npx") {
                has_npm = true;
            }
            if cmd_lower.contains("pip") || cmd_lower.contains("pytest") {
                has_pip = true;
            }
        }
    }

    // فحص: مشروع Python لكن الخطة تحتوي npm
    if let ProjectEnv::Python = env {
        if has_npm && !has_pip {
            return Some(format!(
                "Fatal: Python project but plan contains npm commands (has_npm={}, has_pip={})",
                has_npm, has_pip
            ));
        }
    }

    // فحص: مشروع Node لكن الخطة تحتوي pip
    if let ProjectEnv::Node { .. } = env {
        if has_pip && !has_npm {
            return Some(format!(
                "Fatal: Node project but plan contains pip commands (has_pip={}, has_npm={})",
                has_pip, has_npm
            ));
        }
    }

    None
}

// ============================================================================
// الواجهة الرئيسية
// ============================================================================

/// نقطة الدخول الرئيسية لتطبيق القيود
pub fn apply(
    plan: Vec<Cmd>,
    env: &ProjectEnv,
    state: &ProjectState,
) -> ConstraintResult {
    // 1. فحص fatal أولاً
    if let Some(reason) = check_fatal(&plan, env) {
        return ConstraintResult::Fatal(reason);
    }

    // 2. Environment Lock
    let plan = enforce_environment(plan, env);

    // 3. Config Deduplication - معالجة WriteFile
    let mut processed = Vec::new();
    for cmd in plan {
        match cmd {
            Cmd::WriteFile { ref path, ref content } => {
                if let Some(new_cmd) = intercept_write_file(path, content, state, env) {
                    processed.push(new_cmd);
                }
                // إذا كانت intercept_write_file أعادت None، نتجاهل الأمر (نمنع الكتابة)
            }
            _ => processed.push(cmd),
        }
    }

    // 4. Dependency Normalization
    let plan = normalize_dependencies(processed, env);

    ConstraintResult::Ok(plan)
}

// ============================================================================
// اختبارات الوحدة
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_detect_rust() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(ProjectEnv::detect(dir.path()), ProjectEnv::Rust);
    }

    #[test]
    fn test_detect_python() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();
        assert_eq!(ProjectEnv::detect(dir.path()), ProjectEnv::Python);
    }

    #[test]
    fn test_detect_node() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(
            ProjectEnv::detect(dir.path()),
            ProjectEnv::Node {
                has_typescript: false
            }
        );
    }

    #[test]
    fn test_enforce_environment_python() {
        let env = ProjectEnv::Python;
        let plan = vec![
            Cmd::Run {
                command: "npm install".to_string(),
            },
            Cmd::Run {
                command: "pytest".to_string(),
            },
        ];

        let filtered = enforce_environment(plan, &env);
        assert_eq!(filtered.len(), 1);
        match &filtered[0] {
            Cmd::Run { command } => assert_eq!(command, "pytest"),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_normalize_package_json() {
        let dir = tempdir().unwrap();
        let state = ProjectState {
            has_jest_config: false,
            ..Default::default()
        };
        let env = ProjectEnv::Node {
            has_typescript: true,
        };

        let input = r#"{"name": "test"}"#;
        let output = normalize_package_json(input, &state, &env);

        assert!(output.contains("typescript\": \"5.3.3\""));
        assert!(output.contains("ts-jest\": \"29.1.1\""));
        assert!(output.contains("\"jest\""));
    }

    #[test]
    fn test_remove_jest_when_config_exists() {
        let state = ProjectState {
            has_jest_config: true,
            has_package_json: true,
            jest_in_pkg_json: true,
            ..Default::default()
        };
        let env = ProjectEnv::Node {
            has_typescript: true,
        };

        let input = r#"{"name": "test", "jest": {"preset": "ts-jest"}}"#;
        let output = normalize_package_json(input, &state, &env);

        assert!(!output.contains("\"jest\""));
    }
}

```

## File: context.rs

```rust
// ─────────────────────────────────────────────
// src/context.rs — SEL Agent v5.8
// Context Budget Engine
// ─────────────────────────────────────────────

use crate::chunker::{
    extract_error_locations, get_file_content_smart, SmartContent, MAX_FILE_LINES,
};
use std::fs;
use std::path::{Path, PathBuf};

// ─── الثوابت ───────────────────────────────────

pub const MAX_REPAIR_TOKENS: usize = 8_000;
pub const MAX_CONTEXT_FILES: usize = 50; // v5.1: رفع من 20 إلى 50
const CHARS_PER_TOKEN: usize = 4;
const MIN_SCORE: u8 = 2;
const SMALL_FILE_LINES: usize = 200;

// ─── الأنواع ───────────────────────────────────

#[derive(Debug, Clone)]
pub struct ScoredFile {
    pub path: PathBuf,
    pub content: String,
    pub score: u8,
    pub reasons: Vec<String>,
}

pub struct RepairContext {
    pub stderr: String,
    pub recent_edits: Vec<PathBuf>,
    pub max_tokens: usize,
    pub force_include: Vec<PathBuf>,
    pub culprit_files: Vec<String>, // الملفات المسبّبة للخطأ — أعلى أولوية
    pub context_config: Option<crate::types::ContextConfig>,
}

impl Default for RepairContext {
    fn default() -> Self {
        Self {
            stderr: String::new(),
            recent_edits: vec![],
            max_tokens: MAX_REPAIR_TOKENS,
            force_include: vec![],
            culprit_files: vec![],
            context_config: None,
        }
    }
}

#[derive(Debug)]
pub struct BudgetReport {
    pub total_files: usize,
    pub selected_files: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
}

impl BudgetReport {
    pub fn reduction_pct(&self) -> u8 {
        if self.tokens_before == 0 {
            return 0;
        }
        let saved = self.tokens_before.saturating_sub(self.tokens_after);
        ((saved * 100) / self.tokens_before) as u8
    }

    pub fn print(&self) {
        println!("\n📊 Context Budget:");
        println!(
            "  Files:  {} total → {} selected",
            self.total_files, self.selected_files
        );
        println!(
            "  Tokens: {} → {} (-{}%)",
            self.tokens_before,
            self.tokens_after,
            self.reduction_pct()
        );
    }
}

// ─── الدالة الرئيسية ───────────────────────────

pub fn select_repair_files(
    workspace_files: &[PathBuf],
    ctx: &RepairContext,
) -> (Vec<ScoredFile>, BudgetReport) {
    // 1. اقرأ الملفات وصنفها
    let mut scored: Vec<ScoredFile> = workspace_files
        .iter()
        .filter_map(|path| read_and_score(path, ctx))
        .collect();

    // 2. رتب تنازلياً حسب الـ score
    scored.sort_by(|a, b| b.score.cmp(&a.score));

    let total_files = scored.len();
    let tokens_before = scored.iter().map(|f| estimate_tokens(&f.content)).sum();

    // 3. اختر ضمن حد الـ tokens
    let mut selected = vec![];
    let mut tokens_after = 0usize;

    for file in scored {
        if file.score < MIN_SCORE {
            break; // مرتبة تنازلياً — ما بعدها أقل
        }
        let file_tokens = estimate_tokens(&file.content);
        if tokens_after + file_tokens > ctx.max_tokens {
            break;
        }
        tokens_after += file_tokens;
        selected.push(file);
    }

    // أضف force_include التي لم تُختر بعد
    for path in &ctx.force_include {
        let already = selected.iter().any(|s| &s.path == path);
        if !already {
            if let Some(content) = std::fs::read_to_string(path).ok() {
                let tokens = estimate_tokens(&content);
                if tokens_after + tokens <= ctx.max_tokens {
                    tokens_after += tokens;
                    selected.push(ScoredFile {
                        path: path.clone(),
                        content,
                        score: 0,
                        reasons: vec!["force_include".to_string()],
                    });
                }
            }
        }
    }

    let report = BudgetReport {
        total_files,
        selected_files: selected.len(),
        tokens_before,
        tokens_after,
    };

    (selected, report)
}

// ─── Scoring ───────────────────────────────────

fn read_and_score(path: &Path, ctx: &RepairContext) -> Option<ScoredFile> {
    let raw = fs::read_to_string(path).ok()?;
    let line_count = raw.lines().count();
    let content = if line_count > MAX_FILE_LINES && !ctx.stderr.is_empty() {
        let locs = extract_error_locations(&ctx.stderr);
        if !locs.is_empty() {
            match get_file_content_smart(path, &locs) {
                Ok(SmartContent::Chunk(chunk)) => format!(
                    "// ⚠️ CHUNKED: {} ({} lines, showing {}-{})\n{}",
                    path.display(),
                    line_count,
                    chunk.start_line,
                    chunk.end_line,
                    chunk.content
                ),
                Ok(SmartContent::FullFile(c)) => c,
                Err(_) => raw,
            }
        } else {
            raw
        }
    } else {
        raw
    };
    let (score, reasons) = compute_score(path, &content, ctx);
    Some(ScoredFile {
        path: path.to_path_buf(),
        content,
        score,
        reasons,
    })
}

fn compute_score(path: &Path, content: &str, ctx: &RepairContext) -> (u8, Vec<String>) {
    let mut score = 0u8;
    let mut reasons = vec![];

    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

    // +10 ملف في focus_paths (v5.1)
    if let Some(ref config) = ctx.context_config {
        if config
            .focus_paths
            .iter()
            .any(|fp| path.to_string_lossy().contains(fp))
        {
            score += 10;
            reasons.push("focus path".to_string());
        }
    }

    // +8 ملف مسبّب مباشر (multi-file repair memory)
    if ctx.culprit_files.iter().any(|c| c == filename) {
        score += 8;
        reasons.push("culprit file".to_string());
    }
    // +5 مذكور في stderr
    if ctx.stderr.contains(filename) {
        score += 5;
        reasons.push("mentioned in error".to_string());
    }

    // +3 عُدّل في آخر attempt
    if ctx.recent_edits.contains(&path.to_path_buf()) {
        score += 3;
        reasons.push("recently edited".to_string());
    }

    // +2 يستورد ملفاً فيه خطأ
    if imports_errored_file(content, &ctx.stderr) {
        score += 2;
        reasons.push("imports errored file".to_string());
    }

    // +1 ملف صغير
    if content.lines().count() < SMALL_FILE_LINES {
        score += 1;
        reasons.push("small file".to_string());
    }

    (score, reasons)
}

fn imports_errored_file(content: &str, stderr: &str) -> bool {
    // استخرج أسماء الملفات من stderr
    let errored_stems: Vec<&str> = stderr
        .split_whitespace()
        .filter(|w| w.contains('.'))
        .filter_map(|w| {
            let clean = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '.');
            clean.split('.').next()
        })
        .collect();

    // فحص أسطر الاستيراد في أول 30 سطر
    for stem in &errored_stems {
        for line in content.lines().take(30) {
            let line = line.trim();
            let is_import = line.starts_with("use ")
                || line.starts_with("import ")
                || line.starts_with("from ")
                || line.contains("require(");

            if is_import && line.contains(stem) {
                return true;
            }
        }
    }
    false
}

// ─── Token Estimation ──────────────────────────

pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + CHARS_PER_TOKEN - 1) / CHARS_PER_TOKEN
}

// ─── Tests ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(stderr: &str) -> RepairContext {
        RepairContext {
            stderr: stderr.to_string(),
            force_include: vec![],
            ..Default::default()
        }
    }

    #[test]
    fn test_mentioned_in_error() {
        let ctx = make_ctx("Error in main.rs:42: undefined variable");
        let (score, reasons) = compute_score(Path::new("main.rs"), "fn main() {}", &ctx);
        assert_eq!(score, 6); // 5 + 1 (small)
        assert!(reasons.iter().any(|r| r == "mentioned in error"));
    }

    #[test]
    fn test_unrelated_file() {
        let ctx = make_ctx("Error in main.rs:42");
        let (score, _) = compute_score(Path::new("utils.rs"), "pub fn helper() {}", &ctx);
        assert_eq!(score, 1); // فقط small file
    }

    #[test]
    fn test_recently_edited() {
        let path = PathBuf::from("app.py");
        let ctx = RepairContext {
            stderr: "SyntaxError in db.py".to_string(),
            recent_edits: vec![path.clone()],
            max_tokens: MAX_REPAIR_TOKENS,
            force_include: vec![],
            culprit_files: vec![],
            context_config: None,
        };
        let (score, reasons) = compute_score(&path, "x = 1", &ctx);
        assert_eq!(score, 4); // 3 + 1 (small)
        assert!(reasons.iter().any(|r| r == "recently edited"));
    }

    #[test]
    fn test_python_import_detection() {
        let content = "from db import Database\nclass App:\n    pass";
        assert!(imports_errored_file(content, "Error in db.py:10"));
    }

    #[test]
    fn test_rust_use_detection() {
        let content = "use crate::db;\n\nfn main() {}";
        assert!(imports_errored_file(content, "error in db.rs:5"));
    }

    #[test]
    fn test_token_estimation() {
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }

    #[test]
    fn test_budget_report_reduction() {
        let report = BudgetReport {
            total_files: 10,
            selected_files: 3,
            tokens_before: 8000,
            tokens_after: 1600,
        };
        assert_eq!(report.reduction_pct(), 80);
    }
}

// ─── v5.1: Reference File Support ──────────────

pub fn read_ref_file(ref_file: &Path) -> Option<String> {
    // v5.4: توسيع ~ في المسار
    let expanded = {
        let s = ref_file.to_string_lossy();
        if s.starts_with("~/") {
            if let Ok(home) = std::env::var("HOME") {
                std::path::PathBuf::from(format!("{}/{}", home, &s[2..]))
            } else {
                ref_file.to_path_buf()
            }
        } else {
            ref_file.to_path_buf()
        }
    };
    match std::fs::read_to_string(&expanded) {
        Ok(content) => {
            println!(
                "📄 Loaded ref file: {} ({} lines)",
                ref_file.display(),
                content.lines().count()
            );
            Some(content)
        }
        Err(e) => {
            eprintln!("⚠️  Failed to read ref file: {}", e);
            None
        }
    }
}

```

## File: context_v53_additions.rs

```rust
// ============================================================
// context_v53_additions.rs — مرجع للتعديلات على context.rs
// لا تُضف هذا الملف للـ module tree — هو للقراءة فقط
// طبّق التعديلات يدوياً على context.rs الموجود
// ============================================================
 
// ─── الخطوة أ: أضف هذه الـ imports في أعلى context.rs ───────
 
// use crate::chunker::{
//     extract_error_locations, get_file_content_smart,
//     estimate_context_tokens, SmartContent, MAX_FILE_LINES,
// };
 
// ─── الخطوة ب: البنية الجديدة لنتيجة build_context ───────────
 
pub struct ContextResult {
    pub context_text: String,
    pub chunk_hints: Vec<String>,
    pub estimated_tokens: usize,
    pub used_chunking: bool,
}
 
// ─── الخطوة ج: الدالة الجديدة — أضفها في context.rs ─────────
//
// استدعيها من agent.rs/executor.rs بدل build_context القديمة
// المعاملات الجديدة الوحيدة: test_output
//
// pub fn build_context_v53(
//     workspace: &Path,
//     test_output: &str,          // ← الجديد
//     focus_paths: Option<&[String]>,
//     ref_file: Option<&Path>,
//     max_files: usize,
// ) -> Result<ContextResult, String> {
//
//     let error_locs = extract_error_locations(test_output);
//
//     if !error_locs.is_empty() {
//         eprintln!("[SEL v5.3] مواقع الأخطاء:");
//         for loc in &error_locs {
//             eprintln!("  {} سطر {}", loc.file, loc.line);
//         }
//     }
//
//     let files = collect_workspace_files(workspace, focus_paths, max_files)?;
//     let mut context_parts: Vec<String> = Vec::new();
//     let mut chunk_hints: Vec<String> = Vec::new();
//     let mut used_chunking = false;
//
//     // معالجة --ref-file
//     if let Some(ref_path) = ref_file {
//         let ref_name = ref_path.file_name()
//             .map(|n| n.to_string_lossy().to_string())
//             .unwrap_or("ref-file".into());
//         let smart = get_file_content_smart(ref_path, &error_locs)?;
//         if smart.is_chunk() { used_chunking = true; }
//         if let Some(hint) = smart.context_hint(&ref_name) { chunk_hints.push(hint); }
//         context_parts.push(format!("### [REF] {}\n```\n{}\n```",
//             ref_name, smart.content_for_prompt(&ref_name)));
//     }
//
//     // معالجة ملفات workspace
//     for (file_path, _) in &files {
//         let path = workspace.join(file_path);
//         let smart = get_file_content_smart(&path, &error_locs)
//             .unwrap_or_else(|_| SmartContent::FullFile(
//                 fs::read_to_string(&path).unwrap_or_default()
//             ));
//         if smart.is_chunk() { used_chunking = true; }
//         if let Some(hint) = smart.context_hint(file_path) { chunk_hints.push(hint); }
//         let content = smart.content_for_prompt(file_path);
//         if !content.is_empty() {
//             context_parts.push(format!("### {}\n```\n{}\n```", file_path, content));
//         }
//     }
//
//     let context_text = context_parts.join("\n\n");
//     let estimated_tokens = context_text.len() / 4;
//
//     if used_chunking {
//         eprintln!("[SEL v5.3] chunking مُفعّل → tokens مقدّرة: ~{}", estimated_tokens);
//     }
//
//     Ok(ContextResult { context_text, chunk_hints, estimated_tokens, used_chunking })
// }
 
// ─── الخطوة د: تعديل بناء الـ system prompt في agent.rs ──────
//
// ابحث عن مكان بناء الـ prompt وأضف:
//
// if !context.chunk_hints.is_empty() {
//     system_prompt.push_str("\n\n## ⚠️ ملفات مقطوعة — اقرأ هذا أولاً\n");
//     for hint in &context.chunk_hints {
//         system_prompt.push_str(&format!("- {}\n", hint));
//     }
//     system_prompt.push_str(
//         "\nعند كتابة patch_file، استخدم أرقام الأسطر الظاهرة في الكود.\n\
//          لا تبدأ العد من 1 إذا كان الـ chunk يبدأ من سطر آخر.\n"
//     );
// }

```

## File: environment.rs

```rust
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PythonInfo {
    pub cmd: String,
    pub version: String,
    pub venv: bool,
    pub pip: bool,
}

#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub cmd: String,
    pub version: String,
}

#[derive(Debug, Clone, Default)]
pub struct EnvironmentCapabilities {
    pub python: Option<PythonInfo>,
    pub node: Option<ToolInfo>,
    pub rust: Option<ToolInfo>,
    pub go: Option<ToolInfo>,
    pub git: Option<ToolInfo>,
}

impl EnvironmentCapabilities {
    pub fn probe() -> Self {
        Self {
            python: probe_python(),
            node: probe_tool("node", &["--version"]),
            rust: probe_tool("cargo", &["--version"]),
            go: probe_tool("go", &["version"]),
            git: probe_tool("git", &["--version"]),
        }
    }

    pub fn to_planning_context(&self) -> String {
        let mut lines = vec!["[ENVIRONMENT CAPABILITIES]".to_string()];
        match &self.python {
            Some(p) => lines.push(format!(
                "- Python: {} ({}) | venv:{} pip:{}",
                p.cmd, p.version, p.venv, p.pip
            )),
            None => lines.push("- Python: NOT AVAILABLE".to_string()),
        }
        match &self.node {
            Some(t) => lines.push(format!("- Node.js: available ({})", t.version)),
            None => lines.push("- Node.js: NOT AVAILABLE".to_string()),
        }
        match &self.rust {
            Some(t) => lines.push(format!("- Rust/cargo: available ({})", t.version)),
            None => lines.push("- Rust/cargo: NOT AVAILABLE".to_string()),
        }
        match &self.go {
            Some(t) => lines.push(format!("- Go: available ({})", t.version)),
            None => lines.push("- Go: NOT AVAILABLE".to_string()),
        }
        match &self.git {
            Some(t) => lines.push(format!("- Git: available ({})", t.version)),
            None => lines.push("- Git: NOT AVAILABLE".to_string()),
        }
        lines.join("\n")
    }

    pub fn derive_constraints(&self) -> String {
        let mut lines = vec!["[CONSTRAINTS]".to_string()];
        match &self.python {
            Some(p) => {
                lines.push(format!(
                    "- Use \"{}\" for Python commands (confirmed available)",
                    p.cmd
                ));
                if p.venv {
                    lines.push(format!(
                        "- Use \"{} -m venv venv\" directly — venv module confirmed",
                        p.cmd
                    ));
                } else {
                    lines.push("- venv NOT available — do not plan venv commands".to_string());
                }
            }
            None => {
                lines.push("- Python NOT available — do not plan any python commands".to_string())
            }
        }
        if self.node.is_none() {
            lines.push("- Node.js NOT available — do not plan npm/node commands".to_string());
        }
        if self.go.is_none() {
            lines.push("- Go NOT available — do not plan go commands".to_string());
        }
        lines.join("\n")
    }
}

fn probe_python() -> Option<PythonInfo> {
    for cmd in &["python3", "python"] {
        if let Ok(out) = Command::new(cmd).arg("--version").output() {
            if out.status.success() {
                let raw = String::from_utf8_lossy(&out.stdout);
                let version = raw.trim().to_string();
                let venv = Command::new(cmd)
                    .args(["-m", "venv", "--help"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                let pip = Command::new(cmd)
                    .args(["-m", "pip", "--version"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                return Some(PythonInfo {
                    cmd: cmd.to_string(),
                    version,
                    venv,
                    pip,
                });
            }
        }
    }
    None
}

fn probe_tool(cmd: &str, args: &[&str]) -> Option<ToolInfo> {
    Command::new(cmd).args(args).output().ok().and_then(|out| {
        if out.status.success() {
            let raw = String::from_utf8_lossy(&out.stdout);
            let version = raw.lines().next().unwrap_or("?").trim().to_string();
            Some(ToolInfo {
                cmd: cmd.to_string(),
                version,
            })
        } else {
            None
        }
    })
}

```

## File: evaluator.rs

```rust
// evaluator.rs — v6.1 RAS + DTO
use std::cmp::Ordering;

#[derive(Debug, Clone, Default)]
pub struct RawMetrics {
    pub tests_passed: u32,
    pub tests_total: u32,
    pub mutation_score: f64,
    pub repairs: u32,
    pub retries: u32,
    pub connection_errors: u32,
    pub rate_limits: u32,
    pub timeouts: u32,
    pub elapsed_secs: u64,
}

#[derive(Debug, Clone)]
pub struct ModelScore {
    pub model: String,
    pub run_id: String,
    pub correctness: f64,
    pub reliability: f64,
    pub efficiency: f64,
    pub composite: f64,
    pub unstable: bool,
    pub raw: RawMetrics,
}

impl ModelScore {
    pub fn from_metrics(model: &str, run_id: &str, m: &RawMetrics, max_time: u64) -> Self {
        let correctness = compute_correctness(m);
        let reliability = compute_reliability(m);
        let efficiency = compute_efficiency(m.elapsed_secs, max_time);
        let composite = (correctness * 0.70) + (reliability * 0.20) + (efficiency * 0.10);
        let unstable = reliability < 0.5;
        Self {
            model: model.into(),
            run_id: run_id.into(),
            correctness,
            reliability,
            efficiency,
            composite,
            unstable,
            raw: m.clone(),
        }
    }
}

fn compute_correctness(m: &RawMetrics) -> f64 {
    let test_ratio = if m.tests_total > 0 {
        m.tests_passed as f64 / m.tests_total as f64
    } else {
        0.0
    };
    (test_ratio + m.mutation_score) / 2.0
}

fn compute_reliability(m: &RawMetrics) -> f64 {
    let penalty = (m.connection_errors as f64 * 0.15)
        + (m.rate_limits as f64 * 0.10)
        + (m.retries as f64 * 0.05)
        + (m.timeouts as f64 * 0.12)
        + (m.repairs as f64 * 0.03);
    (1.0 - penalty).max(0.0)
}

fn compute_efficiency(elapsed: u64, max_time: u64) -> f64 {
    if max_time == 0 {
        return 1.0;
    }
    // إذا كان الفرق أقل من 20% → كلاهما متساويان عملياً
    let ratio = elapsed as f64 / max_time as f64;
    if ratio >= 0.80 {
        // الأبطأ يحصل على 0.80 كحد أدنى معقول
        let penalty = (ratio - 0.80) * 2.0; // penalty بطيء
        (1.0 - penalty).clamp(0.70, 1.0)
    } else {
        // الأسرع يحصل على bonus
        (1.0 - ratio * 0.5).clamp(0.70, 1.0)
    }
}

// DTO — Deterministic Total Ordering (5 مستويات)
pub fn rank_models(mut scores: Vec<ModelScore>) -> Vec<ModelScore> {
    scores.sort_by(|a, b| {
        // UNSTABLE يخسر دائماً
        match (a.unstable, b.unstable) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        // مستوى 1: correctness
        b.correctness
            .partial_cmp(&a.correctness)
            .unwrap_or(Ordering::Equal)
            // مستوى 2: reliability
            .then_with(|| {
                b.reliability
                    .partial_cmp(&a.reliability)
                    .unwrap_or(Ordering::Equal)
            })
            // مستوى 3: efficiency
            .then_with(|| {
                b.efficiency
                    .partial_cmp(&a.efficiency)
                    .unwrap_or(Ordering::Equal)
            })
            // مستوى 4: اسم النموذج
            .then_with(|| a.model.cmp(&b.model))
            // مستوى 5: run_id
            .then_with(|| a.run_id.cmp(&b.run_id))
    });
    scores
}

// عرض جدول المقارنة
pub fn print_comparison_table(scores: &[ModelScore]) {
    println!("\n{}", "═".repeat(80));
    println!("  Model Comparison — v6.1 Reliability-Aware Scoring");
    println!("{}", "═".repeat(80));
    println!(
        "  {:<28} {:>8} {:>9} {:>9} {:>9}  {}",
        "Model", "Correct", "Reliable", "Effic.", "Composite", "Status"
    );
    println!("{}", "─".repeat(80));

    for (i, s) in scores.iter().enumerate() {
        let winner = if i == 0 && !s.unstable {
            "✓ Winner"
        } else {
            ""
        };
        let status = if s.unstable { "⚠ UNSTABLE" } else { winner };
        println!(
            "  {:<28} {:>7.2} {:>8.2} {:>9.2} {:>9.3}  {}",
            s.model, s.correctness, s.reliability, s.efficiency, s.composite, status
        );
        println!(
            "  {:<28} repairs:{} retries:{} conn_err:{} rate_lim:{}",
            "", s.raw.repairs, s.raw.retries, s.raw.connection_errors, s.raw.rate_limits
        );
        println!("{}", "─".repeat(80));
    }
}

```

## File: executor.rs

```rust
// src/executor.rs — v0.4: تنفيذ آمن
use std::collections::HashMap;

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::process::Command as TCmd;
use crate::types::{ExecResult, SafetyError};
use crate::protocol::Cmd;

const ALLOWED: &[&str] = &[
    "python3", "python",
    "venv/bin/python3", "venv/bin/python",
    "venv/bin/pip3",    "venv/bin/pip",
    "venv/bin/uvicorn", "venv/bin/gunicorn",
    "venv/bin/pytest",  "pytest",
    "node", "npm", "npx", "node_modules/.bin/jest",
    "cargo", "rustc", "git",
    "go",
    "mkdir", "touch", "ls", "cat", "cp", "mv",
    "echo", "find", "grep", "curl", "chmod",
    "node", "npm",
];

const BLOCKED: &[&str] = &[
    "sudo", "rm -rf", "mkfs", "dd if=",
    "| sh", "| bash", "curl | bash",
    "> /dev/", "/etc/", "/sys/", "/proc/",
];

pub struct SafeExecutor {
    pub workspace: PathBuf,
    timeout_secs: u64,
    patch_attempts: std::cell::RefCell<HashMap<PathBuf, usize>>, // v5.2: track patch failures
}

fn fix_rust_string_literals(src: &str) -> String {
    let mut result = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'\'' && bytes[end] != b'\n' {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'\'' && end > start + 1 {
                let word = &src[start..end];
                if word.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                    result.push('"');
                    result.push_str(word);
                    result.push('"');
                    i = end + 1;
                    continue;
                }
            }
        }
        let ch = src[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}


impl SafeExecutor {
    pub fn new(workspace: PathBuf, timeout_secs: u64) -> Self {
        Self { 
            workspace, 
            timeout_secs,
            patch_attempts: std::cell::RefCell::new(HashMap::new()),
        }
    }

    pub async fn run(&self, cmd: &Cmd) -> Result<ExecResult> {
        match cmd {
            Cmd::Run       { command }         => self.shell(command).await,
            Cmd::WriteFile { path, content }   => self.write_file(path, content),
            Cmd::AppendFile{ path, content }   => self.append_file(path, content),
            Cmd::DeleteFile{ path }               => self.delete_file(path),
            Cmd::PatchFile { path, search, replace } => self.patch_file(path, search, replace),
            Cmd::ReadFile  { path }            => self.read_file(path),
            Cmd::Mkdir     { path }            => self.mkdir(path),
            Cmd::RunTests  { target }          => self.run_tests(target).await,
            Cmd::Done      { .. }              => Ok(ExecResult::ok("done")),
        }
    }

    // ─── Shell ─────────────────────────────────────

    async fn shell(&self, command: &str) -> Result<ExecResult> {
        self.safety_check(command)?;

        let parts: Vec<&str> = command.split_whitespace().collect();
        let prog = parts.first().ok_or_else(|| anyhow!("Empty command"))?;

        // رفض pip install بدون package name
        if prog.contains("pip3") || prog.contains("pip") {
            let is_install = parts.iter().any(|p| *p == "install");
            let has_package = parts.len() > 2 && parts.iter().skip(2).any(|p| !p.starts_with('-'));
            if is_install && !has_package {
                return Ok(ExecResult::fail(
                    "pip install needs package name: e.g. venv/bin/pip3 install pytest".to_string()
                ));
            }
        }

        if !ALLOWED.iter().any(|a| *a == *prog) {
            return Ok(ExecResult::fail(format!(
                "'{}' is not in the allowed programs list", prog
            )));
        }

        let services = ["venv/bin/uvicorn", "uvicorn", "venv/bin/gunicorn"];
        if services.contains(prog) {
            return self.service(prog, &parts[1..]).await;
        }

        let start = Instant::now();
        let out = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            TCmd::new(prog).args(&parts[1..]).current_dir(&self.workspace).output(),
        ).await
        .map_err(|_| anyhow!("Timeout after {}s: {}", self.timeout_secs, command))??;

        Ok(ExecResult {
            success:     out.status.success(),
            exit_code:   out.status.code().unwrap_or(-1),
            stdout:      String::from_utf8_lossy(&out.stdout).into(),
            stderr:      String::from_utf8_lossy(&out.stderr).into(),
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn service(&self, prog: &str, args: &[&str]) -> Result<ExecResult> {
        println!("   🌐 Service: {}", prog);
        TCmd::new(prog).args(args).current_dir(&self.workspace).spawn()?;
        tokio::time::sleep(Duration::from_millis(800)).await;
        Ok(ExecResult::ok("Service started"))
    }

    // ─── File Operations ───────────────────────────

    fn write_file(&self, path: &str, content: &str) -> Result<ExecResult> {
        // 🛡️ Hard Language Wall: منع كتابة ملفات بلغة مختلفة
        // FIX: Language Guard متماثل وكامل
        {
            let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_config = matches!(path, "Cargo.toml" | "go.mod" | "go.sum" | "package.json"
                | "tsconfig.json" | "Makefile" | ".gitignore" | "README.md");

            if !is_config && !ext.is_empty() {
                let is_rust = self.workspace.join("Cargo.toml").exists();
                let is_go   = self.workspace.join("go.mod").exists();
                let is_ts   = self.workspace.join("tsconfig.json").exists();
                let is_py   = self.workspace.join("setup.py").exists()
                    || self.workspace.join("pyproject.toml").exists()
                    || self.workspace.join("requirements.txt").exists();

                let workspace_lang = if is_rust { Some("Rust") }
                    else if is_go   { Some("Go") }
                    else if is_ts   { Some("TypeScript") }
                    else if is_py   { Some("Python") }
                    else            { None };

                let allowed_ext: &[&str] = if is_rust     { &["rs", "toml"] }
                    else if is_go   { &["go", "mod", "sum", "sh"] }
                    else if is_ts   { &["ts", "tsx", "js", "json"] }
                    else if is_py   { &["py", "txt", "cfg", "toml", "ini"] }
                    else            { &[] };

                if let Some(lang) = workspace_lang {
                    if !allowed_ext.is_empty() && !allowed_ext.contains(&ext) {
                        return Ok(ExecResult::fail(format!(
                            "LANGUAGE LOCK BLOCKED: Cannot write '.{}' file '{}' in a {} workspace.                              Use {} files only.",
                            ext, path, lang, lang
                        )));
                    }
                }
            }
        }
        let p = self.safe_path(path)?;
        // حماية: ملفات محمية لا يُكتب عليها إذا كانت موجودة
        let protected = ["Cargo.toml", "Cargo.lock", "go.mod", "go.sum"];
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if protected.contains(&name) && p.exists() {
            return Ok(ExecResult::fail(format!(
                "write_file: '{}' is protected — use patch_file to modify existing files", path
            )));
        }
        if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
        // حماية: إذا كان الملف موجوداً وأكبر بكثير من المحتوى الجديد → تحذير
        if p.exists() {
            let existing_len = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            let new_len = content.len() as u64;
            if existing_len > 500 && new_len < existing_len / 3 {
                println!("   ⚠ WARNING: Overwriting {} ({} bytes) with much smaller content ({} bytes)",
                    path, existing_len, new_len);
            }
        }
        // Rust brace balance check
        let content = if path.ends_with(".rs") {
            let open  = content.chars().filter(|&c| c == '{').count();
            let close = content.chars().filter(|&c| c == '}').count();
            if open > close {
                let mut fixed = content.to_string();
                for _ in 0..(open - close) { fixed.push_str("
}"); }
                std::borrow::Cow::Owned(fixed)
            } else {
                std::borrow::Cow::Borrowed(content)
            }
        } else {
            std::borrow::Cow::Borrowed(content)
        };
        // v5.5: auto-fix single-quote string literals in Rust files
        let content = if p.extension().map(|x| x == "rs").unwrap_or(false) {
            std::borrow::Cow::Owned(fix_rust_string_literals(content.as_ref()))
        } else {
            content
        };
        // v7.3: sanitize Unicode quotes قبل الكتابة
        let content_str = sanitize_code(content.as_ref());
        eprintln!("[TRACE] write_file sanitize: input={} output={}", content.as_ref().len(), content_str.len());
        std::fs::write(&p, content_str.as_bytes())?;
        // Auto-fix: إذا كُتب jest.config.js → احذف "jest" field من package.json
        if path.ends_with("jest.config.js") {
            let pkg = self.workspace.join("package.json");
            if pkg.exists() {
                if let Ok(pkg_src) = std::fs::read_to_string(&pkg) {
                    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&pkg_src) {
                        if v.get("jest").is_some() {
                            v.as_object_mut().unwrap().remove("jest");
                            if let Ok(fixed) = serde_json::to_string_pretty(&v) {
                                let _ = std::fs::write(&pkg, fixed);
                                println!("   🔧 AutoFix: removed jest field from package.json (conflicts with jest.config.js)");
                            }
                        }
                    }
                }
            }
        }
        println!("   📝 {} ({} bytes)", path, content.len());
        // v7.3: compile check فوري بعد write_file
        if path.ends_with(".go") {
            // AutoFix: undefined Go stdlib import بدون LLM
            if let Some(err) = go_compile_check(&self.workspace) {
                // حاول AutoFix أولاً
                eprintln!("[TRACE] Checking autofix for: {}", &err[..std::cmp::min(80, err.len())]);
                if let Some(fixed) = autofix_go_undefined_import(&p, &err) {
                    println!("   🔧 AutoFix Go import: {}", fixed);
                    // أعد الفحص بعد الإصلاح
                    if go_compile_check(&self.workspace).is_none() {
                        println!("   ✅ AutoFix succeeded");
                    } else if let Some(err2) = go_compile_check(&self.workspace) {
                        return Ok(ExecResult::fail(format!(
                            "COMPILE ERROR in '{}' — NOTE: The actual error might be in a DIFFERENT file. Check the error details below and fix the file mentioned there:\n{}",
                            path, err2
                        )));
                    }
                } else {
                    return Ok(ExecResult::fail(format!(
                        "COMPILE ERROR in '{}' — NOTE: The actual error might be in a DIFFERENT file. Check the error details below and fix the file mentioned there:\n{}",
                        path, err
                    )));
                }
            }
        }
        Ok(ExecResult::ok(format!("Written: {}", path)))
    }

    fn append_file(&self, path: &str, content: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        if !p.exists() {
            return Ok(ExecResult::fail(format!("'{}' not found — use write_file first", path)));
        }
        let mut orig = std::fs::read_to_string(&p)?;
        if !orig.ends_with('\n') { orig.push('\n'); }
        orig.push('\n');
        orig.push_str(content);
        std::fs::write(&p, &orig)?;
        println!("   ➕ {} (+{} bytes)", path, content.len());
        Ok(ExecResult::ok(format!("Appended: {}", path)))
    }

    fn delete_file(&self, path: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        // حماية: لا نحذف ملفات الإعداد الجذرية
        let protected = ["Cargo.toml", "go.mod", "package.json", "Cargo.lock"];
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if protected.contains(&name) {
            return Ok(ExecResult::fail(format!(
                "delete_file: '{}' is protected and cannot be deleted", path
            )));
        }
        // حماية: لا نحذف مجلدات
        if p.is_dir() {
            return Ok(ExecResult::fail(format!(
                "delete_file: '{}' is a directory — only files allowed", path
            )));
        }
        if !p.exists() {
            return Ok(ExecResult::ok(format!("delete_file: '{}' already absent", path)));
        }
        std::fs::remove_file(&p)?;
        println!("   🗑  Deleted: {}", path);
        Ok(ExecResult::ok(format!("Deleted: {}", path)))
    }


    // v5.2: Validate patch result to prevent code corruption
    fn validate_patch(&self, path: &str, original: &str, patched: &str) -> Result<(), String> {
        // 1. Sanity checks for common corruption patterns
        if patched.contains("}ype") || patched.contains("{ype") {
            return Err("Suspicious patch: corrupted type keyword detected".to_string());
        }
        
        if patched.contains("#\\[") || patched.contains("#\\]") {
            return Err("Invalid Rust escape: backslash in attribute syntax detected".to_string());
        }
        
        // 2. Line count sanity check
        let orig_lines = original.lines().count();
        let new_lines = patched.lines().count();
        let diff = (new_lines as i32 - orig_lines as i32).abs();
        
        let max_allowed = (orig_lines * 2).max(50) as i32;
        if diff > max_allowed {
            return Err(format!("Patch changed too many lines: {} → {} lines", orig_lines, new_lines));
        }
        
        // 3. Rust-specific checks
        if path.ends_with(".rs") {
            // Check for unmatched braces (basic)
            let open_braces = patched.matches('{').count();
            let close_braces = patched.matches('}').count();
            if open_braces != close_braces {
                return Err(format!("Unmatched braces: {} open, {} close", open_braces, close_braces));
            }
        }
        
        Ok(())
    }



    fn patch_file(&self, path: &str, search: &str, replace: &str) -> Result<ExecResult> {
        // FIX: Language Guard متماثل في patch_file
        {
            let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_config = matches!(path, "Cargo.toml" | "go.mod" | "go.sum" | "package.json"
                | "tsconfig.json" | "Makefile" | ".gitignore" | "README.md");

            if !is_config && !ext.is_empty() {
                let is_rust = self.workspace.join("Cargo.toml").exists();
                let is_go   = self.workspace.join("go.mod").exists();
                let is_ts   = self.workspace.join("tsconfig.json").exists();
                let is_py   = self.workspace.join("setup.py").exists()
                    || self.workspace.join("pyproject.toml").exists()
                    || self.workspace.join("requirements.txt").exists();

                let workspace_lang = if is_rust { Some("Rust") }
                    else if is_go   { Some("Go") }
                    else if is_ts   { Some("TypeScript") }
                    else if is_py   { Some("Python") }
                    else            { None };

                let allowed_ext: &[&str] = if is_rust     { &["rs", "toml"] }
                    else if is_go   { &["go", "mod", "sum", "sh"] }
                    else if is_ts   { &["ts", "tsx", "js", "json"] }
                    else if is_py   { &["py", "txt", "cfg", "toml", "ini"] }
                    else            { &[] };

                if let Some(lang) = workspace_lang {
                    if !allowed_ext.is_empty() && !allowed_ext.contains(&ext) {
                        return Ok(ExecResult::fail(format!(
                            "LANGUAGE LOCK BLOCKED: Cannot patch '.{}' file '{}' in a {} workspace.                              Use {} files only.",
                            ext, path, lang, lang
                        )));
                    }
                }
            }
        }
        let p = self.safe_path(path)?;
        
        // v5.2: Fallback to write_file after 2 failed patch attempts
        {
            let mut attempts = self.patch_attempts.borrow_mut();
            let count = *attempts.entry(p.clone()).or_insert(0);
            
            if count >= 2 {
                println!("   ⚠️  patch_file failed {} times on '{}' — switching to write_file fallback", count, path);
                drop(attempts); // release borrow
                
                // Read current content and apply replacement manually
                let content = std::fs::read_to_string(&p)?;
                let content = sanitize_code(&content);
                let search_buf = sanitize_code(search);
                let search = search_buf.as_str();
                let replace_buf = sanitize_code(replace);
                let replace = replace_buf.as_str();
                // FIX(Opus): تحقق من وجود search قبل الكتابة — منع Silent Corruption
                if !content.contains(search) {
                    self.patch_attempts.borrow_mut().insert(p.clone(), 0);
                    return Ok(ExecResult::fail(format!(
                        "PATCH FAILED: search block not found in '{}' after {} attempts — use write_file with complete file content",
                        path, count
                    )));
                }
                let new_content = content.replacen(search, replace, 1);
                let new_content = sanitize_code(&new_content);
                std::fs::write(&p, new_content.as_bytes())?;
                self.patch_attempts.borrow_mut().insert(p.clone(), 0);
                return Ok(ExecResult::ok(format!(
                    "patch_file: fallback write_file applied to '{}' successfully",
                    path
                )));
            }
        }
        
        if !p.exists() {
            return Ok(ExecResult::fail(format!("patch_file: '{}' not found — use write_file to create it first", path)));
        }
        if search.trim().is_empty() {
            return Ok(ExecResult::fail("patch_file: search block is empty".to_string()));
        }
        let content = std::fs::read_to_string(&p)?;
        let content = sanitize_code(&content);
        let count = content.matches(search).count();
        // إذا لم يُوجد مباشرة — جرب normalize whitespace
        let (effective_search, effective_replace, normalized) = if count == 0 {
            let norm_content = content.split_whitespace().collect::<Vec<_>>().join(" ");
            let norm_search  = search.split_whitespace().collect::<Vec<_>>().join(" ");
            let norm_replace = replace.split_whitespace().collect::<Vec<_>>().join(" ");
            (norm_content, norm_replace, Some(norm_search))
        } else {
            (content.clone(), replace.to_string(), None)
        };
        let (search_key, content_key) = if let Some(ref ns) = normalized {
            (ns.as_str(), effective_search.as_str())
        } else {
            (search, content.as_str())
        };
        let count = content_key.matches(search_key).count();
        if count == 0 {
            // v5.2: increment failure counter
            *self.patch_attempts.borrow_mut().entry(p.clone()).or_insert(0) += 1;
            return Ok(ExecResult::fail(format!(
                "patch_file: search block not found in '{}' (tried exact + whitespace-normalized) — copy the exact text from the file", path
            )));
        }
        if count > 1 {
            return Ok(ExecResult::fail(format!(
                "patch_file: search block found {} times in '{}' — must be unique, use more context", count, path
            )));
        }
        let new_content = if normalized.is_some() {
            content_key.replacen(search_key, &effective_replace, 1)
        } else {
            content.replacen(search, replace, 1)
        };
        // v5.2: Validate before writing
        if let Err(e) = self.validate_patch(path, &content, &new_content) {
            *self.patch_attempts.borrow_mut().entry(p.clone()).or_insert(0) += 1;
            return Ok(ExecResult::fail(format!("patch_file validation failed: {}", e)));
        }
        
        // v5.5: auto-fix single-quote string literals in Rust files
        let new_content = if p.extension().map(|x| x == "rs").unwrap_or(false) {
            fix_rust_string_literals(&new_content)
        } else {
            new_content
        };
        std::fs::write(&p, &new_content)?;

        // v5.2: reset counter on success
        self.patch_attempts.borrow_mut().insert(p.clone(), 0);
        
        println!("   🔧 patch_file: {} ({} bytes → {} bytes)", path, content.len(), new_content.len());
        Ok(ExecResult::ok(format!("Patched: {}", path)))
    }

    fn read_file(&self, path: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        if !p.exists() {
            return Ok(ExecResult::fail(format!("'{}' not found", path)));
        }
        let content = std::fs::read_to_string(&p)?;
        println!("   📖 {} ({} bytes)", path, content.len());
        Ok(ExecResult { success: true, exit_code: 0, stdout: content, stderr: String::new(), duration_ms: 0 })
    }

    fn mkdir(&self, path: &str) -> Result<ExecResult> {
        std::fs::create_dir_all(self.workspace.join(path))?;
        Ok(ExecResult::ok(format!("mkdir: {}", path)))
    }

    // ─── RunTests ──────────────────────────────────

    async fn run_tests(&self, target: &str) -> Result<ExecResult> {
        // 🛡️ Language Guard: منع تشغيل اختبارات بلغة مختلفة عن لغة المشروع
        let is_rust = self.workspace.join("Cargo.toml").exists();
        let is_go = self.workspace.join("go.mod").exists();
        if (is_rust || is_go) && target.ends_with(".py") {
            return Ok(ExecResult::fail(format!(
                "LANGUAGE LOCK BLOCKED: Attempted to run Python test '{}' in a Rust/Go workspace. Stick to the project language.", target
            )));
        }
        // Auto-detect: إذا طُلب cargo لكن لا Cargo.toml → تحقق من Go
        let target = if target == "cargo" && !self.workspace.join("Cargo.toml").exists() {
            let has_go = std::fs::read_dir(&self.workspace)
                .map(|d| d.flatten().any(|e| e.path().extension()
                    .and_then(|x| x.to_str()) == Some("go")))
                .unwrap_or(false);
            if has_go { "go" } else { target }
        } else { target };

        // Rust tests
        if target.ends_with(".rs") || target == "cargo" {
            println!("   🦀 cargo test");
            let start = std::time::Instant::now();
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new("cargo")
                    .args(["test", "--", "--nocapture"])
                    .current_dir(&self.workspace)
                    .output(),
            ).await
            .map_err(|_| anyhow!("cargo test timeout"))??;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{}\n{}", stdout, stderr);
            // SHA2 Digest Auto-fix v1.4
            if combined.contains("trait `Digest` which provides") {
                println!("   🔧 AutoFix: adding sha2::Digest import");
                for dir in [self.workspace.as_path(), self.workspace.join("src").as_path()] {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                                if let Ok(content) = std::fs::read_to_string(&p) {
                                    if content.contains("sha2::Sha256") && !content.contains("use sha2::Digest") {
                                        let _ = std::fs::write(&p, format!("use sha2::Digest;\n{}", content));
                                        println!("   ✅ Fixed {:?}", p.file_name().unwrap_or_default());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let exit_ok = out.status.success();
            let (passed, failed) = parse_rust_tests(&combined);
            let success = exit_ok && passed > 0;
            if success { println!("   ✅ Tests passed (exit 0)"); }
            else if exit_ok && passed == 0 { println!("   ❌ Tests FAILED — 0 tests ran"); }
            else       { println!("   ❌ Tests FAILED (exit {})", out.status.code().unwrap_or(-1)); }
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success { String::new() } else {
                    let start = combined.len().saturating_sub(2000);
                    combined[start..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // Go tests
        if target.ends_with(".go") || target == "go" {
            println!("   🐹 go test ./...");
            // Auto-fix: run go mod tidy before tests if go.mod exists
            if self.workspace.join("go.mod").exists() {
                let _ = TCmd::new("go")
                    .args(["mod", "tidy"])
                    .current_dir(&self.workspace)
                    .output().await;
                println!("   🔧 AutoFix: go mod tidy done");
            }
            
            let start = std::time::Instant::now();
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new("go")
                    .args(["test", "./...", "-v"])
                    .current_dir(&self.workspace)
                    .output(),
            ).await
            .map_err(|_| anyhow!("go test timeout"))??;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{}\n{}", stdout, stderr);
            let exit_ok = out.status.success();
            let (passed, failed) = parse_go_tests(&combined);
            let success = exit_ok && passed > 0;
            if success { println!("   ✅ Tests passed (exit 0)"); }
            else if exit_ok && passed == 0 { println!("   ❌ Tests FAILED — 0 tests ran"); }
            else       { println!("   ❌ Tests FAILED (exit {})", out.status.code().unwrap_or(-1)); }
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success { String::new() } else {
                    let start = combined.len().saturating_sub(2000);
                    combined[start..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // Node.js tests — npm test / jest / npx jest
        // اكتشاف TypeScript من الـ workspace (package.json موجود)
        let is_ts_workspace = self.workspace.join("package.json").exists();
        let is_node_target = target.ends_with(".js")
            || target.ends_with(".ts")   // v6.6: .ts files → npm test لا pytest
            || target == "npm test"
            || target == "npm"
            || target == "jest"
            || target == "npx jest"
            || target.contains("jest")
            || is_ts_workspace;          // v6.6: أي workspace فيه package.json → npm test
        if is_node_target {
            // تحديد الأمر الصحيح
            let (prog, args): (&str, Vec<&str>) = if target == "jest" {
                // شغّل jest مباشرة من node_modules
                ("npx", vec!["jest", "--runInBand", "--forceExit"])
            } else if target == "npx jest" {
                ("npx", vec!["jest", "--runInBand", "--forceExit"])
            } else if target == "npm test" || target == "npm" {
                ("npm", vec!["test", "--", "--runInBand", "--forceExit"])
            } else if is_ts_workspace || target.ends_with(".ts") {
                // v7.4: TypeScript files MUST be executed by jest, never directly by node
                let t_str = Box::leak(target.to_string().into_boxed_str());
                ("npx", vec!["jest", t_str, "--runInBand", "--forceExit"])
            } else {
                // .js file — شغّل مع node
                let t_str = Box::leak(target.to_string().into_boxed_str());
                ("node", vec![t_str])
            };
            println!("   🟨 {} {}", prog, args.join(" "));
            let start = std::time::Instant::now();
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new(prog).args(&args).current_dir(&self.workspace).output(),
            ).await
            .map_err(|_| anyhow!("Node.js test timeout after {}s", self.timeout_secs))??;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{}
{}", stdout, stderr);
            let exit_ok = out.status.success();
            // parse Jest results
            let passed = combined.lines()
                .filter(|l| l.contains("✓") || l.contains("✔") || l.contains("passed"))
                .count();
            let success = exit_ok && passed > 0;
            if success { println!("   ✅ Tests passed (exit 0)"); }
            else if exit_ok && passed == 0 { println!("   ❌ Tests FAILED — 0 tests ran"); }
            else       { println!("   ❌ Tests FAILED (exit {})", out.status.code().unwrap_or(-1)); }
            let failed = combined.lines()
                .filter(|l| l.contains("✗") || l.contains("✘") || l.contains("failed") || l.contains("FAIL"))
                .count();
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed
{}", passed, failed, &stdout[..stdout.len().min(1000)]),
                stderr: if success { String::new() } else {
                    let start = combined.len().saturating_sub(2000);
                    combined[start..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // v5.6: Auto-create venv إذا لم يكن موجوداً
        if !self.workspace.join("venv").exists() {
            println!("   🔧 AutoFix: creating venv...");
            let _ = tokio::process::Command::new("python3")
                .args(["-m", "venv", "venv"])
                .current_dir(&self.workspace)
                .output().await;
            if self.workspace.join("venv").exists() {
                let _ = tokio::process::Command::new("venv/bin/pip")
                    .args(["install", "pytest", "-q"])
                    .current_dir(&self.workspace)
                    .output().await;
            }
        }
        // Auto-install pytest في venv إذا لم يكن موجوداً
        if self.workspace.join("venv").exists()
            && !self.workspace.join("venv/bin/pytest").exists() {
            println!("   🔧 AutoFix: installing pytest in venv...");
            let _ = tokio::process::Command::new("venv/bin/pip")
                .args(["install", "pytest", "-q"])
                .current_dir(&self.workspace)
                .output().await;
        }
        let pytest = if self.workspace.join("venv/bin/pytest").exists() {
            "venv/bin/pytest"
        } else { "pytest" };


        // StdlibConflict Pre-check v1.3
        {
            let conflicts = ["numbers","decimal","types","typing","abc","queue",
                             "math","string","io","re","json","csv",
                             "random","time","collections","functools",
                             "itertools","pathlib","enum","copy"];
            for name in &conflicts {
                let f = self.workspace.join(format!("{}.py", name));
                if f.exists() {
                    println!("   Removing {}.py (stdlib conflict)", name);
                    let _ = std::fs::remove_file(&f);
                }
            }
        }
        // Clean the target if the LLM wrongly passed the entire command
        let mut clean_target = target;
        if clean_target.starts_with("venv/bin/pytest ") {
            clean_target = &clean_target["venv/bin/pytest ".len()..];
        } else if clean_target.starts_with("pytest ") {
            clean_target = &clean_target["pytest ".len()..];
        } else if clean_target == "venv/bin/pytest" || clean_target == "pytest" {
            clean_target = "";
        }
        let clean_target = clean_target.replace("-v", "").replace("--tb=short", "").trim().to_string();

        let t = if clean_target.is_empty() || clean_target == "." { String::new() } else { format!(" {}", clean_target) };
        let cmd = format!("{}{} -v --tb=short", pytest, t);
        println!("   🧪 {}", cmd);

        let out = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            TCmd::new(pytest)
                .args(if clean_target.is_empty() || clean_target == "." { vec!["-v", "--tb=short"] } else { vec![&clean_target, "-v", "--tb=short"] })
                .current_dir(&self.workspace)
                .env("PYTHONPATH", &self.workspace)
                .output(),
        ).await
        .map_err(|_| anyhow!("pytest timeout"))??;

        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let combined = format!("{}\n{}", stdout, stderr);

        let (passed, failed) = parse_pytest(&combined);

        // Smart Success Detection v1.3
        // exit code وحده غير كافٍ — بعض الأدوات (PyQt6-WebEngine) تُرجع -1 مع اختبارات ناجحة
        // القاعدة الآمنة: نجاح فقط إذا passed>0 و failed==0 و لا يوجد error في الـ summary
        let has_passed  = passed > 0;
        let has_failed  = failed > 0;
        let has_error   = combined.contains("ERROR collecting")
                       || combined.contains("error during collection")
                       || combined.contains("errors in collection");
        let collected_zero = combined.contains("collected 0 items")
                          || combined.contains("no tests ran");

        let success = if has_passed && !has_failed && !has_error && !collected_zero {
            // الاختبارات نجحت — تجاهل exit code غير الصفري من أدوات خارجية
            if out.status.code().unwrap_or(0) != 0 && out.status.code() != Some(5) {
                println!("   ⚠ exit code {} — ignored (tests passed cleanly)", out.status.code().unwrap_or(-1));
            }
            true
        } else {
            // لا توجد passed أو يوجد فشل — اعتمد exit code
            out.status.success() && out.status.code() != Some(5)
        };

        if success { println!("   ✅ Tests passed (exit 0)"); }
        else       { println!("   ❌ Tests FAILED (exit {})", out.status.code().unwrap_or(-1)); }

        Ok(ExecResult {
            success,
            exit_code:   out.status.code().unwrap_or(-1),
            stdout:      format!("{} passed, {} failed", passed, failed),
            stderr:      if success {
                String::new()
            } else {
                let tail_start = combined.len().saturating_sub(2000);
                combined[tail_start..].to_string()
            },
            duration_ms: 0,
        })
    }

    // ─── Helpers ───────────────────────────────────

    fn safe_path(&self, path: &str) -> Result<PathBuf> {
        if path.contains("..") {
            return Err(anyhow!(SafetyError::PathTraversal(path.to_string()).to_string()));
        }
        let full = self.workspace.join(path);
        if !full.starts_with(&self.workspace) {
            return Err(anyhow!(SafetyError::WorkspaceEscape(path.to_string()).to_string()));
        }
        Ok(full)
    }

    fn safety_check(&self, cmd: &str) -> Result<()> {
        let lower = cmd.to_lowercase();
        for b in BLOCKED {
            if lower.contains(b) {
                return Err(anyhow!(SafetyError::BlockedCommand(b.to_string()).to_string()));
            }
        }
        Ok(())
    }
}

fn parse_pytest(output: &str) -> (usize, usize) {
    let mut passed = 0;
    let mut failed = 0;
    for line in output.lines().rev() {
        if line.contains(" passed") || line.contains(" failed") {
            // السطر: "=== 2 passed, 1 failed in 0.03s ==="
            // نبحث عن الرقم قبل كل كلمة مفتاحية
            for seg in line.split(',') {
                let s = seg.trim();
                let words: Vec<&str> = s.split_whitespace().collect();
                for (i, w) in words.iter().enumerate() {
                    if *w == "passed" && i > 0 {
                        if let Ok(n) = words[i-1].parse::<usize>() { passed = n; }
                    }
                    if *w == "failed" && i > 0 {
                        if let Ok(n) = words[i-1].parse::<usize>() { failed = n; }
                    }
                }
            }
            break;
        }
    }
    (passed, failed)
}


#[derive(Debug, PartialEq)]
pub enum MutationResult {
    Strong,
    Weak(String, String),  // (original_line, mutated_line)
    Skipped,
}

fn apply_all_mutations(code: &str) -> Vec<(String, String, String)> {
    let strategies: &[(&str, &str)] = &[
        ("==", "!="), ("!=", "=="),
        (" > ", " < "), (" < ", " > "),
        (" >= ", " <= "), (" <= ", " >= "),
        ("return True", "return False"), ("return False", "return True"),
        (" + ", " - "), (" - ", " + "),
    ];
    let skip_patterns = ["i += ", "i -= ", "j += ", "j -= ",
                          "idx", "index", "len(", "range(", "count +=", "count -="];
    let mut result = Vec::new();
    for (from, to) in strategies {
        let mut found_line = None;
        let mutated: String = code.lines()
            .map(|line| {
                let trimmed = line.trim_start();
                let skip = trimmed.starts_with('#') || trimmed.starts_with("//")
                        || skip_patterns.iter().any(|p| line.contains(p));
                if found_line.is_none() && !skip && line.contains(*from) {
                    let new_line = line.replacen(from, to, 1);
                    found_line = Some((line.trim().to_string(), new_line.trim().to_string()));
                    new_line
                } else { line.to_string() }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Some((orig, mutd)) = found_line {
            result.push((mutated, orig, mutd));
        }
    }
    result
}

impl SafeExecutor {
    pub async fn mutation_check(&self, source_file: &str) -> MutationResult {
        let source_path = self.workspace.join(source_file);
        if !source_path.exists() { return MutationResult::Skipped; }
        let ext = source_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let original = match std::fs::read_to_string(&source_path) {
            Ok(s) => s, Err(_) => return MutationResult::Skipped,
        };
        let mutations = apply_all_mutations(&original);
        if mutations.is_empty() { return MutationResult::Skipped; }
        // test runner per language
        let test_cmd: Vec<String> = match ext {
            "py" => {
                let pytest = if self.workspace.join("venv/bin/pytest").exists() {
                    "venv/bin/pytest"
                } else { "pytest" };
                vec![pytest.into(), "-x".into(), "-q".into(), "--tb=no".into()]
            }
            "go" => vec!["go".into(), "test".into(), "./...".into(), "-count=1".into()],
            "js" | "ts" => {
                let npx = if self.workspace.join("node_modules/.bin/jest").exists() {
                    "node_modules/.bin/jest"
                } else { "npx" };
                vec![npx.into(), "--forceExit".into(), "--silent".into()]
            }
            "rs" => vec!["cargo".into(), "test".into(), "--quiet".into()],
            _ => return MutationResult::Skipped,
        };
        let mut survived_orig = String::new();
        let mut survived_mutd = String::new();
        let mut any_caught  = false;
        let mut any_missed  = false;
        for (mutation, orig_line, mutd_line) in &mutations {
            if std::fs::write(&source_path, mutation).is_err() {
                let _ = std::fs::write(&source_path, &original);
                continue;
            }
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                tokio::process::Command::new(&test_cmd[0])
                    .args(&test_cmd[1..])
                    .current_dir(&self.workspace)
                    .env("PYTHONPATH", &self.workspace)
                    .output(),
            ).await;
            let _ = std::fs::write(&source_path, &original);
            match out {
                Ok(Ok(result)) => {
                    if result.status.success() {
                        if !any_missed {
                            survived_orig = orig_line.clone();
                            survived_mutd = mutd_line.clone();
                        }
                        any_missed = true;
                    } else { any_caught = true; }
                }
                _ => {}
            }
            if any_missed { break; }
        }
        let _ = std::fs::write(&source_path, &original);
        if any_missed { MutationResult::Weak(survived_orig, survived_mutd) }
        else if any_caught { MutationResult::Strong }
        else { MutationResult::Skipped }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn ex(dir: &std::path::Path) -> SafeExecutor {
        SafeExecutor::new(dir.to_path_buf(), 10)
    }

    #[test]
    fn write_and_read() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        e.write_file("a.py", "x=1").unwrap();
        let r = e.read_file("a.py").unwrap();
        assert_eq!(r.stdout, "x=1");
    }

    #[test]
    fn append_file() {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join("a.py"), "line1\n").unwrap();
        let e = ex(d.path());
        let r = e.append_file("a.py", "line2").unwrap();
        assert!(r.success);
        let c = std::fs::read_to_string(d.path().join("a.py")).unwrap();
        assert!(c.contains("line1") && c.contains("line2"));
    }

    #[test]
    fn blocks_path_traversal() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        assert!(e.write_file("../../etc/passwd", "x").is_err());
    }

    #[tokio::test]
    async fn runs_echo() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        let r = e.run(&Cmd::Run { command: "echo hello".into() }).await.unwrap();
        assert!(r.success);
        assert!(r.stdout.contains("hello"));
    }
}

fn parse_rust_tests(output: &str) -> (usize, usize) {
    let mut total_passed = 0usize;
    let mut total_failed = 0usize;
    for line in output.lines() {
        if line.contains("test result:") {
            for seg in line.split(';') {
                let s = seg.trim();
                let words: Vec<&str> = s.split_whitespace().collect();
                for (i, w) in words.iter().enumerate() {
                    if *w == "passed" && i > 0 {
                        if let Ok(n) = words[i-1].parse::<usize>() { total_passed += n; }
                    }
                    if *w == "failed" && i > 0 {
                        if let Ok(n) = words[i-1].parse::<usize>() { total_failed += n; }
                    }
                }
            }
        }
    }
    (total_passed, total_failed)
}

fn parse_go_tests(output: &str) -> (usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    for line in output.lines() {
        if line.starts_with("--- PASS") { passed += 1; }
        if line.starts_with("--- FAIL") { failed += 1; }
    }
    (passed, failed)
}

fn go_compile_check(workspace: &std::path::Path) -> Option<String> {
    if !workspace.join("go.mod").exists() { 
        eprintln!("[TRACE] go_compile_check: skipped (no go.mod)");
        return None; 
    }
    eprintln!("[TRACE] go_compile_check: running...");
    let out = std::process::Command::new("go")
        .args(&["test", "-run=^$", "-count=1"])
        .current_dir(workspace)
        .output()
        .ok()?;
    if !out.status.success() {
        Some(String::from_utf8_lossy(&out.stderr).to_string())
    } else {
        None
    }
}

/// AutoFix: يضيف Go stdlib import تلقائياً بدون LLM
fn autofix_go_undefined_import(file: &std::path::Path, err: &str) -> Option<String> {
    let go_std: &[(&str, &str)] = &[
        ("fmt",     "fmt"),
        ("errors",  "errors"),
        ("strings", "strings"),
        ("strconv", "strconv"),
        ("sort",    "sort"),
        ("math",    "math"),
        ("os",      "os"),
        ("io",      "io"),
        ("log",     "log"),
        ("time",    "time"),
        ("sync",    "sync"),
        ("context", "context"),
        ("bytes",   "bytes"),
        ("bufio",   "bufio"),
    ];

    // استخرج الرمز من "undefined: fmt"
    let sym = err.lines()
        .find(|l| l.contains("undefined:"))?
        .split("undefined:")
        .nth(1)?
        .trim()
        .split_whitespace()
        .next()?
        .split('.')
        .next()?
        .to_string();

    // تحقق أنه stdlib
    let pkg = go_std.iter()
        .find(|(name, _)| *name == sym.as_str())
        .map(|(_, pkg)| *pkg)?;

    // اقرأ الملف
    let src = std::fs::read_to_string(file).ok()?;

    // إذا كان موجوداً بالفعل
    if src.contains(&format!("\"{}\"", pkg)) {
        return None;
    }

    // أضف import
    let new_src = if src.contains("import (") {
        src.replacen("import (", &format!("import (\n\t\"{}\"", pkg), 1)
    } else {
        // أضف بعد package declaration
        let pkg_line = src.lines()
            .find(|l| l.starts_with("package "))?
            .to_string();
        src.replacen(
            &pkg_line,
            &format!("{}\n\nimport \"{}\"", pkg_line, pkg),
            1,
        )
    };

    std::fs::write(file, &new_src).ok()?;
    Some(pkg.to_string())
}

/// تنظيف الكود من Unicode quotes
fn sanitize_code(s: &str) -> String {
    let original_len = s.len();
    let result = s
        .replace('\u{201C}', "\"")
        .replace('\u{201D}', "\"")
        .replace('\u{2018}', "'")
        .replace('\u{2019}', "'")
        .replace('\u{2014}', "--")
        .replace('\u{2013}', "-")
        .replace('\u{00A0}', " ")
        .replace('\u{200B}', "")
        .replace('\u{FEFF}', "")
        .to_string();
    
    if result.len() != original_len {
        eprintln!("[TRACE] sanitize_code: cleaned {} Unicode chars", 
                  original_len - result.len());
    }
    result
}



#[cfg(test)]
mod bench_bugs {
    use super::*;
    use std::fs;

    fn make_ws(name: &str) -> PathBuf {
        let ws = std::env::temp_dir().join(format!("sel_bench_{}", name));
        let _ = fs::remove_dir_all(&ws);
        fs::create_dir_all(&ws).unwrap();
        ws
    }

    /// B1: sanitize_code يُصلح Unicode Quotes → ASCII
    #[test]
    fn b1_sanitize_unicode_quotes() {
        let input = "\u{201C}hello\u{201D}";  // "hello" (Unicode)
        let result = sanitize_code(input);
        assert_eq!(result, "\"hello\"", "Unicode quotes must become ASCII");
    }

    /// B2: patch_file يفشل صراحةً عند غياب search block
    #[test]
    fn b2_patch_explicit_fail() {
        let ws = make_ws("b2");
        fs::write(ws.join("main.rs"), "fn main() {}").unwrap();
        
        let exec = SafeExecutor::new(ws.clone(), 60);
        let result = exec.patch_file("main.rs", "GHOST_TEXT", "NEW").unwrap();
        
        assert!(!result.success, "patch_file must fail when search block is missing");
        assert!(result.stderr.contains("not found") || result.stderr.contains("FAILED"),
            "Error should mention 'not found', got: {}", result.stderr);
    }

    /// B4-a: Language Guard يمنع write_file لملفات .py في Rust workspace
    #[test]
    fn b4_language_wall_write() {
        let ws = make_ws("b4w");
        fs::write(ws.join("Cargo.toml"), "[package]\nname=\"t\"\n").unwrap();
        
        let exec = SafeExecutor::new(ws, 60);
        let result = exec.write_file("calc.py", "x = 1").unwrap();
        
        assert!(!result.success, "write_file must reject .py in Rust workspace");
        assert!(result.stderr.contains("LANGUAGE LOCK") || result.stderr.contains("BLOCKED"),
            "Must contain LANGUAGE LOCK, got: {}", result.stderr);
    }

    /// B4-b: Language Guard يمنع patch_file لملفات .py في Go workspace
    #[test]
    fn b4_language_wall_patch() {
        let ws = make_ws("b4p");
        fs::write(ws.join("go.mod"), "module test\n").unwrap();
        fs::write(ws.join("calc.py"), "x = 1\n").unwrap();
        
        let exec = SafeExecutor::new(ws, 60);
        let result = exec.patch_file("calc.py", "x = 1", "x = 2").unwrap();
        
        assert!(!result.success, "patch_file must reject .py in Go workspace");
        assert!(result.stderr.contains("LANGUAGE LOCK") || result.stderr.contains("BLOCKED"),
            "Must contain LANGUAGE LOCK, got: {}", result.stderr);
    }
}

```

## File: goal_parser.rs

```rust
// goal_parser.rs — v6.5
// يحوّل goal النصي إلى ParsedGoal منظم
// ScaffoldEngine يستخدمه بدلاً من string matching المتفرق

use crate::scaffold_engine::ProjectKind;

// ══════════════════════════════════════════════════════
// SubKind — نوع المشروع الفرعي
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum SubKind {
    // Python
    Flask,
    FastAPI,
    Django,
    // TypeScript/Node
    Express,
    React,
    // None
    Plain,
}

// ══════════════════════════════════════════════════════
// ParsedGoal — ناتج التحليل
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ParsedGoal {
    pub kind: ProjectKind,
    pub sub_kind: SubKind,
    pub extra_deps: Vec<String>, // حزم إضافية للتثبيت في Scaffold
}

// ══════════════════════════════════════════════════════
// parse() — نقطة الدخول الوحيدة
// ══════════════════════════════════════════════════════

pub fn parse(workspace: &std::path::Path, goal: &str) -> ParsedGoal {
    let kind = detect_kind(workspace, goal);
    let sub_kind = detect_sub_kind(&kind, goal);
    let extra_deps = detect_extra_deps(&kind, &sub_kind, goal);

    ParsedGoal {
        kind,
        sub_kind,
        extra_deps,
    }
}

// ─── اكتشاف ProjectKind ───────────────────────────────

fn detect_kind(workspace: &std::path::Path, goal: &str) -> ProjectKind {
    // من ملفات موجودة أولاً
    if workspace.join("Cargo.toml").exists() {
        return ProjectKind::Rust;
    }
    if workspace.join("go.mod").exists() {
        return ProjectKind::Go;
    }
    if workspace.join("package.json").exists() {
        return ProjectKind::TypeScript;
    }
    if workspace.join("requirements.txt").exists() || workspace.join("pyproject.toml").exists() {
        return ProjectKind::Python;
    }

    // من الـ goal
    let g = goal.to_lowercase();
    if g.contains("typescript")
        || g.contains(" ts ")
        || g.contains(".ts")
        || g.contains("express")
        || g.contains("react")
        || g.contains("jest")
    {
        return ProjectKind::TypeScript;
    }
    if g.contains("python")
        || g.contains("pytest")
        || g.contains("flask")
        || g.contains("fastapi")
        || g.contains("django")
    {
        return ProjectKind::Python;
    }
    if g.contains("rust") || g.contains("cargo") {
        return ProjectKind::Rust;
    }
    if g.contains("golang") || g.contains(" go ") || g.contains("gorilla") {
        return ProjectKind::Go;
    }

    ProjectKind::Unknown
}

// ─── اكتشاف SubKind ───────────────────────────────────

fn detect_sub_kind(kind: &ProjectKind, goal: &str) -> SubKind {
    let g = goal.to_lowercase();
    match kind {
        ProjectKind::Python => {
            if g.contains("fastapi") || g.contains("fast api") {
                SubKind::FastAPI
            } else if g.contains("flask") {
                SubKind::Flask
            } else if g.contains("django") {
                SubKind::Django
            } else {
                SubKind::Plain
            }
        }
        ProjectKind::TypeScript => {
            if g.contains("express") {
                SubKind::Express
            } else if g.contains("react") {
                SubKind::React
            } else {
                SubKind::Plain
            }
        }
        _ => SubKind::Plain,
    }
}

// ─── اكتشاف extra_deps ────────────────────────────────

fn detect_extra_deps(kind: &ProjectKind, sub_kind: &SubKind, goal: &str) -> Vec<String> {
    let g = goal.to_lowercase();
    
    // v7.4 Fix: Bypass extra_deps extraction for QuickFix tests
    if g.contains("do not use pip_install") || g.contains("do not use pip install") || g.contains("strict rule") {
        return vec![];
    }

    let mut deps: Vec<String> = vec![];

    match kind {
        ProjectKind::Python => {
            match sub_kind {
                SubKind::FastAPI => {
                    deps.push("fastapi".into());
                    deps.push("uvicorn[standard]".into());
                    deps.push("httpx".into()); // TestClient يحتاجه
                }
                SubKind::Flask => {
                    deps.push("flask".into());
                }
                SubKind::Django => {
                    deps.push("django".into());
                    deps.push("pytest-django".into());
                }
                _ => {
                    // اكتشاف إضافي من النص
                    if g.contains("requests") {
                        deps.push("requests".into());
                    }
                    if g.contains("sqlalchemy") {
                        deps.push("sqlalchemy".into());
                    }
                    if g.contains("pydantic") {
                        deps.push("pydantic".into());
                    }
                }
            }
        }
        ProjectKind::TypeScript => match sub_kind {
            SubKind::Express => {
                deps.push("express".into());
                deps.push("@types/express".into());
                deps.push("supertest".into());
                deps.push("@types/supertest".into());
            }
            SubKind::React => {
                deps.push("react".into());
                deps.push("react-dom".into());
                deps.push("@types/react".into());
            }
            _ => {}
        },
        _ => {}
    }

    deps
}

// ══════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fake_ws() -> &'static Path {
        Path::new("/tmp")
    }

    #[test]
    fn test_fastapi_detection() {
        let g = parse(
            fake_ws(),
            "Create a Python FastAPI application with /hello route",
        );
        assert_eq!(g.kind, ProjectKind::Python);
        assert_eq!(g.sub_kind, SubKind::FastAPI);
        assert!(g.extra_deps.contains(&"fastapi".to_string()));
        assert!(g.extra_deps.contains(&"uvicorn[standard]".to_string()));
    }

    #[test]
    fn test_flask_detection() {
        let g = parse(
            fake_ws(),
            "Create a Flask app with a /hello route and pytest tests",
        );
        assert_eq!(g.sub_kind, SubKind::Flask);
        assert!(g.extra_deps.contains(&"flask".to_string()));
    }

    #[test]
    fn test_express_detection() {
        let g = parse(
            fake_ws(),
            "Create a TypeScript Express API with /status endpoint",
        );
        assert_eq!(g.kind, ProjectKind::TypeScript);
        assert_eq!(g.sub_kind, SubKind::Express);
        assert!(g.extra_deps.contains(&"express".to_string()));
    }

    #[test]
    fn test_plain_python() {
        let g = parse(fake_ws(), "Create a Python calculator with pytest tests");
        assert_eq!(g.sub_kind, SubKind::Plain);
        assert!(g.extra_deps.is_empty());
    }

    #[test]
    fn test_rust_no_deps() {
        let g = parse(fake_ws(), "Create a Rust function that adds two numbers");
        assert_eq!(g.kind, ProjectKind::Rust);
        assert_eq!(g.sub_kind, SubKind::Plain);
        assert!(g.extra_deps.is_empty());
    }
}

```

## File: llm.rs

```rust
// This file is left intentionally empty. All LLM logic has been moved to llm_engine.rs.
// Please delete this file safely.

```

## File: llm_engine.rs

```rust
// src/llm_engine.rs — v7.1 Unified LLM Engine
// كيان واحد يدير كل شيء: detection + display + cascade

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use crate::types::Message;

// ─── System Prompt ────────────────────────────────────────────────────────────

const SYSTEM_PROMPT: &str = r#"You are SEL Agent, an autonomous execution engine.
CRITICAL: Respond ONLY with a valid JSON object matching the schema below. No markdown text outside the JSON block.

SCHEMA:
{"version":"1.0","commands":[
  {"type":"run","command":"..."},
  {"type":"write_file","path":"...","content":"..."},
  {"type":"run_tests","target":"..."},
  {"type":"patch_file","path":"...","search":"...","replace":"..."},
  {"type":"done","message":"..."}
]}

RULES:
- Python: Use venv/bin/pytest
- Rust: run_tests target MUST be "cargo"
- Go: run_tests target MUST be "go", ALWAYS import "fmt"/"errors" if used
- TS/Node: run_tests target MUST be "npm test"
- STRONG TESTS: Write comprehensive tests with both positive and negative cases.
"#;

// ─── Stats ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct LlmCallStats {
    pub retries: u32,
    pub connection_errors: u32,
    pub rate_limits: u32,
    pub timeouts: u32,
    pub total_latency_ms: u64,
}

// ─── Provider ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Provider {
    name:        String,
    model:       String,
    endpoint:    String,
    api_key:     String,
    daily_limit: String,
}

impl Provider {
    fn key_preview(&self) -> String {
        let k = &self.api_key;
        if k.len() > 12 {
            format!("{}...{}", &k[..8], &k[k.len()-4..])
        } else if k.len() > 4 {
            format!("{}...", &k[..4])
        } else {
            "***".to_string()
        }
    }
}

// ─── LlmEngine ───────────────────────────────────────────────────────────────

pub struct LlmEngine {
    providers: Vec<Provider>,
    pub stats: LlmCallStats,
}

// ─── Serde types ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Request {
    model:       String,
    messages:    Vec<ApiMsg>,
    temperature: f32,
    max_tokens:  u32,
}

#[derive(Serialize, Deserialize, Clone)]
struct ApiMsg {
    role:    String,
    content: String,
}

#[derive(Deserialize)]
struct Response {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ApiMsg,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlmProvider {
    Groq,
    Moonshot,
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub provider: LlmProvider,
    pub model_id: String,
    pub base_url: String,
    pub env_key: String,
}

impl ModelConfig {
    pub fn from_alias(alias: &str) -> Self {
        match alias {
            "kimi" | "kimi-k2" | "kimi-k2-instruct" => ModelConfig {
                provider: LlmProvider::Groq,
                model_id: "moonshotai/kimi-k2-instruct-0905".to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key: "GROQ_API_KEY".to_string(),
            },
            "llama" | "llama-70b" => ModelConfig {
                provider: LlmProvider::Groq,
                model_id: "llama-3.3-70b-versatile".to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key: "GROQ_API_KEY".to_string(),
            },
            "kimi-k2.5" | "kimi25" | "kimi-latest" => ModelConfig {
                provider: LlmProvider::Moonshot,
                model_id: "kimi-k2.5".to_string(),
                base_url: "https://api.moonshot.ai/v1/chat/completions".to_string(),
                env_key: "MOONSHOT_API_KEY".to_string(),
            },
            "silicon" | "kimi-silicon" => ModelConfig {
                provider: LlmProvider::Moonshot,
                model_id: "moonshotai/Kimi-K2.5".to_string(),
                base_url: "https://api.siliconflow.cn/v1/chat/completions".to_string(),
                env_key: "SILICONFLOW_API_KEY".to_string(),
            },
            _ => ModelConfig {
                provider: LlmProvider::Groq,
                model_id: alias.to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key: "GROQ_API_KEY".to_string(),
            },
        }
    }
}

pub fn classify_json_error(reason: &str) -> String {
    if reason.contains("No ```json") || reason.contains("json block") {
        "⚠️  [النموذج] رد بنص بدل JSON — إعادة بـ prompt مبسط".to_string()
    } else if reason.contains("missing field") {
        "⚠️  [النموذج] JSON ناقص حقل مطلوب".to_string()
    } else {
        format!(
            "⚠️  [النموذج] فشل تحليل JSON — {}",
            &reason[..reason.len().min(50)]
        )
    }
}

// ─── impl LlmEngine ──────────────────────────────────────────────────────────

impl LlmEngine {
    /// يبني المحرك من متغيرات البيئة — الأولوية:
    ///   1. SEL_API_BASE + SEL_API_KEY  → custom/OpenRouter
    ///   2. GEMINI_API_KEY              → Gemini direct
    ///   3. GROQ_API_KEY                → Groq
    pub fn from_env() -> Self {
        let mut providers = Vec::new();

        // 1. Cerebras — الأولوية الأولى (بلا حد يومي، سريع جداً)
        if let Ok(key) = std::env::var("CEREBRAS_API_KEY") {
            if !key.trim().is_empty() {
                let model = std::env::var("CEREBRAS_MODEL")
                    .unwrap_or_else(|_| "qwen-3-235b-a22b-instruct-2507".to_string());
                providers.push(Provider {
                    name:        "Cerebras".to_string(),
                    model,
                    endpoint:    "https://api.cerebras.ai/v1/chat/completions".to_string(),
                    api_key:     key.trim().to_string(),
                    daily_limit: "بلا حد يومي معلن".to_string(),
                });
            }
        }

        // 2. Gemini — احتياطي (1500 RPD مجاناً)
        if let Ok(key) = std::env::var("GEMINI_API_KEY") {
            let key = key.trim().to_string();
            if !key.is_empty() {
                let model = std::env::var("GEMINI_MODEL")
                    .unwrap_or_else(|_| "gemini-2.0-flash".to_string());
                providers.push(Provider {
                    name:        "Gemini".to_string(),
                    model,
                    endpoint:    "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".to_string(),
                    api_key:     key,
                    daily_limit: "1,500 RPD (free tier)".to_string(),
                });
            }
        }

        // 3. Groq — احتياطي ثالث
        if let Ok(key) = std::env::var("GROQ_API_KEY") {
            let key = key.trim().to_string();
            if !key.is_empty() {
                let raw = std::env::var("GROQ_MODEL")
                    .unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());
                let (model, limit) = groq_model_limit(&raw);
                let name = if model.contains("kimi") { "Groq/Kimi" } else { "Groq/Llama" };
                providers.push(Provider {
                    name:        name.to_string(),
                    model,
                    endpoint:    "https://api.groq.com/openai/v1/chat/completions".to_string(),
                    api_key:     key,
                    daily_limit: limit,
                });
            }
        }

        // 4. Custom endpoint (OpenRouter أو أي provider) — آخر احتياطي
        if let (Ok(base), Ok(key)) = (
            std::env::var("SEL_API_BASE"),
            std::env::var("SEL_API_KEY"),
        ) {
            if !base.is_empty() && !key.is_empty() {
                let model = std::env::var("OPENROUTER_MODEL")
                    .unwrap_or_else(|_| "qwen/qwen3-coder:free".to_string());
                let endpoint = normalize_endpoint(&base);
                let name = detect_name(&endpoint);
                let limit = detect_limit(&endpoint, &model);
                providers.push(Provider { name, model, endpoint, api_key: key, daily_limit: limit });
            }
        }

        Self { providers, stats: LlmCallStats::default() }
    }

    /// يعرض معلومات الـ provider الأساسي فقط
    pub fn print_info(&self) {
        if self.providers.is_empty() {
            println!("   ❌ لا يوجد API key صالح");
            println!("   💡 جرّب: export GEMINI_API_KEY=... أو GROQ_API_KEY=...");
            return;
        }

        let p = &self.providers[0];
        println!("   🔌 Provider:  {}", p.name);
        println!("   🤖 Model:     {}", p.model);
        println!("   🔑 Key:       {}", p.key_preview());
        println!("   📊 Limit:     {}", p.daily_limit);

        let secs = secs_to_midnight();
        println!("   ⏰ Resets in: {}h {}m (UTC midnight)", secs / 3600, (secs % 3600) / 60);

        if self.providers.len() > 1 {
            let fallbacks: Vec<&str> = self.providers[1..].iter().map(|p| p.name.as_str()).collect();
            println!("   🔄 Fallback:  {}", fallbacks.join(" → "));
        }
    }


    /// يستدعي LLM بـ cascade تلقائي — 3 جولات مع انتظار تزايدي
    pub async fn call(&mut self, messages: &[Message]) -> Result<String> {
        if self.providers.is_empty() {
            return Err(anyhow!(
                "❌ لا يوجد API key — جرّب: export GEMINI_API_KEY=... او GROQ_API_KEY=..."
            ));
        }

        let msgs = build_msgs(messages);
        let count = self.providers.len();
        let mut daily_exhausted: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let wait_secs = [30u64, 60u64];

        for round in 0..3u32 {
            let mut last_err = None;
            let mut tried_any = false;

            for i in 0..count {
                let provider = self.providers[i].clone();
                if daily_exhausted.contains(&provider.name) {
                    continue;
                }
                tried_any = true;

                match call_one(&provider, &msgs, &mut self.stats).await {
                    Ok(text) => {
                        if i > 0 || round > 0 {
                            println!("   OK {} succeeded", provider.name);
                        }
                        return Ok(text);
                    }
                    Err(e) => {
                        let s = e.to_string();
                        let is_429 = s.contains("429");
                        let is_daily = is_429 && (
                            s.contains("per day") ||
                            s.contains("TPD") ||
                            s.contains("daily") ||
                            s.contains("quota") ||
                            s.contains("exceeded your current quota") ||
                            s.contains("GenerateRequestsPerDay")
                        );
                        let is_auth =
                            s.contains("401") || s.contains("403") ||
                            s.contains("Unauthorized") ||
                            s.contains("Authentication") ||
                            s.contains("Invalid API");

                        if is_daily {
                            println!("   ⛔ {} — daily limit reached", provider.name);
                            daily_exhausted.insert(provider.name.clone());
                        } else if is_429 || is_auth {
                            let reason = if is_429 { "Rate Limit" } else { "Auth" };
                            println!("   🔀 SWITCH {} ({}) → trying next...", provider.name, reason);
                        }
                        last_err = Some(e);
                    }
                }
            }

            if !tried_any { break; }

            if round < 2 {
                let wait = wait_secs[round as usize];
                println!("   ⏳ WAIT: round {}/3 failed — pausing {}s before retry...", round + 1, wait);
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            } else {
                let exhausted: Vec<&str> = self.providers.iter()
                    .filter(|p| daily_exhausted.contains(&p.name))
                    .map(|p| p.name.as_str())
                    .collect();
                let rate_limited: Vec<&str> = self.providers.iter()
                    .filter(|p| !daily_exhausted.contains(&p.name))
                    .map(|p| p.name.as_str())
                    .collect();

                let mut report = String::from("All providers failed after 3 rounds:\n");
                if !exhausted.is_empty() {
                    report.push_str(&format!("  daily exhausted: {}\n", exhausted.join(", ")));
                }
                if !rate_limited.is_empty() {
                    report.push_str(&format!("  rate limited: {}\n", rate_limited.join(", ")));
                }
                report.push_str("Solutions:\n  1. Wait 1 hour\n  2. ollama pull qwen2.5-coder:7b\n  3. Add paid API key\n");

                return Err(last_err
                    .unwrap_or_else(|| anyhow!("All providers failed"))
                    .context(report));
            }
        }

        Err(anyhow!("All providers failed"))
    }

    pub fn call_stats(&self) -> &LlmCallStats {
        &self.stats
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn normalize_endpoint(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/chat/completions") {
        b.to_string()
    } else {
        format!("{}/chat/completions", b)
    }
}

fn detect_name(endpoint: &str) -> String {
    if endpoint.contains("openrouter")  { "OpenRouter".to_string() }
    else if endpoint.contains("nvidia") { "NVIDIA".to_string() }
    else if endpoint.contains("together") { "Together".to_string() }
    else                                { "Custom".to_string() }
}

fn detect_limit(endpoint: &str, _model: &str) -> String {
    if endpoint.contains("openrouter") { "مدفوع — بلا حد يومي".to_string() }
    else                               { "حسب الخطة".to_string() }
}

fn groq_model_limit(raw: &str) -> (String, String) {
    match raw {
        "kimi" | "kimi-k2" | "kimi-k2-instruct" =>
            ("moonshotai/kimi-k2-instruct".to_string(), "300,000 tokens/day".to_string()),
        "llama" | "llama-70b" =>
            ("llama-3.3-70b-versatile".to_string(), "500,000 tokens/day".to_string()),
        "qwen" | "qwen3" =>
            ("qwen/qwen3-32b".to_string(), "حسب الخطة".to_string()),
        "llama4" | "llama-4" =>
            ("meta-llama/llama-4-scout-17b-16e-instruct".to_string(), "حسب الخطة".to_string()),
        _ =>
            ("llama-3.3-70b-versatile".to_string(), "500,000 tokens/day".to_string()),
    }
}

fn secs_to_midnight() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    86400 - (now % 86400)
}

fn classify_err(s: &str) -> &'static str {
    if s.contains("401") || s.contains("403") || s.contains("Unauthorized") { "Auth" }
    else if s.contains("429") { "Rate Limit" }
    else { "Error" }
}

fn build_msgs(messages: &[Message]) -> Vec<ApiMsg> {
    let mut msgs = vec![ApiMsg {
        role:    "system".into(),
        content: SYSTEM_PROMPT.into(),
    }];
    for m in messages {
        msgs.push(ApiMsg { role: m.role.clone(), content: m.content.clone() });
    }
    msgs
}

async fn call_one(
    provider: &Provider,
    msgs:     &[ApiMsg],
    stats:    &mut LlmCallStats,
) -> Result<String> {
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    let delays = [15u64, 45, 120];

    for (attempt, &delay) in delays.iter().enumerate() {
        if attempt > 0 {
            stats.retries += 1;
            println!("   ⏳ retry in {}s...", delay);
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        }

        let req = client.post(&provider.endpoint).json(&Request {
            model:       provider.model.clone(),
            messages:    msgs.to_vec(),
            temperature: 0.1,
            max_tokens:  8192,
        });

        // كل providers تستخدم Bearer (بما فيها Gemini OpenAI-compat)
        let req = req.bearer_auth(&provider.api_key);

        let resp = match req.send().await {
            Ok(r)  => r,
            Err(e) => {
                stats.connection_errors += 1;
                if attempt + 1 == delays.len() {
                    stats.total_latency_ms = start.elapsed().as_millis() as u64;
                    return Err(anyhow!("Connection error: {}", e));
                }
                println!("   ⚠️  Network error — retry in {}s...", delay);
                continue;
            }
        };

        let status = resp.status();

        // 429 → ارجع فوراً للـ cascade (لا تُعيد المحاولة على نفس provider)
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            stats.rate_limits += 1;
            let body = resp.text().await.unwrap_or_default();
            stats.total_latency_ms = start.elapsed().as_millis() as u64;
            return Err(anyhow!("API {} — {}", status, &body[..body.len().min(200)]));
        }

        // server errors (502/503/500) → retry على نفس provider
        if status == reqwest::StatusCode::BAD_GATEWAY
        || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        || status == reqwest::StatusCode::INTERNAL_SERVER_ERROR {
            stats.connection_errors += 1;
            if attempt + 1 == delays.len() {
                let body = resp.text().await.unwrap_or_default();
                stats.total_latency_ms = start.elapsed().as_millis() as u64;
                return Err(anyhow!("API {} — {}", status, &body[..body.len().min(200)]));
            }
            println!("   ⚠  Server error ({}) — retry in {}s...", status, delay);
            continue;
        }

        // auth errors → لا تعيد المحاولة، ارجع مباشرة للـ cascade
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            stats.total_latency_ms = start.elapsed().as_millis() as u64;
            return Err(anyhow!("API {} — {}", status, &body[..body.len().min(200)]));
        }

        let data: Response = resp.json().await?;
        stats.total_latency_ms = start.elapsed().as_millis() as u64;
        return data.choices.into_iter().next()
            .map(|c| c.message.content)
            .ok_or_else(|| anyhow!("Empty response"));
    }

    stats.total_latency_ms = start.elapsed().as_millis() as u64;
    Err(anyhow!("Max retries exceeded"))
}

impl LlmEngine {
    pub fn primary_name(&self) -> String {
        self.providers.first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

```

## File: main.rs

```rust
#![allow(dead_code)]
mod bench_realworld;
mod bench_compile;
mod llm_engine;
// src/main.rs — SEL Agent v7.3.0
mod agent;
mod chunker;
mod context;
mod environment;
mod evaluator;
mod executor;
mod goal_parser;

mod memory;
mod protocol;
mod scaffold_engine;
mod scanner;
mod manifest;
mod types;
mod constitution;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "sel-agent", version = "7.3.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        goal: String,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
        #[arg(long, default_value = "false")]
        dry_run: bool,
        #[arg(long)]
        ref_file: Option<PathBuf>,
        #[arg(long, value_delimiter = ',')]
        focus: Vec<String>,
    },
    Health,
    Stress {
        #[arg(long, default_value = "3")]
        max_repairs: u8,
    },
    Bench {
        #[arg(long, default_value = "all")]
        suite: String,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
        #[arg(long, default_value = "1")]
        iterations: u8,
    },
    Scan {
        /// مسار المشروع
        #[arg(long, default_value = ".")]
        workspace: String,
        /// إخراج JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Compare {
        #[arg(long, value_delimiter = ',')]
        models: Vec<String>,
        #[arg(long, default_value = "python")]
        suite: String,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
    },
    Plan {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
    },
    #[command(name = "bench-real-world")]
    BenchRealWorld {
        #[arg(long)]
        tier: Option<u8>,
        #[arg(long, default_value = "6")]
        max_repairs: u8,
    },
}

async fn run_health(api_key: &str) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent v7.3.0 — Health Check                   ║");
    println!("╚══════════════════════════════════════════╝\n");
    // Provider info في الـ bench
    {
        let mdl = std::env::var("SEL_MODEL").unwrap_or_else(|_| "kimi".to_string());
        let (_ep, _key) = if let Ok(base) = std::env::var("SEL_API_BASE") {
            let k = std::env::var("SEL_API_KEY").unwrap_or_default();
            (base, k)
        } else if mdl.contains("gemini") || mdl.starts_with("models/") {
            let k = std::env::var("GEMINI_API_KEY").unwrap_or_default();
            (
                "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
                k,
            )
        } else {
            let k = std::env::var("GROQ_API_KEY").unwrap_or_default();
            ("https://api.groq.com/openai/v1".to_string(), k)
        };
        // print_provider_info removed(&ep, &mdl, &key);
        println!();
    }

    let internet = reqwest::Client::new()
        .get("https://1.1.1.1")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    match internet {
        Ok(_) => println!("🌐 Internet:     {}", "✅ Connected".green()),
        Err(_) => println!("🌐 Internet:     {}", "❌ No connection".red()),
    }

    let groq = reqwest::Client::new()
        .get("https://api.groq.com")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    match groq {
        Ok(_) => println!("🔌 Groq Server:  {}", "✅ Reachable".green()),
        Err(_) => println!("🔌 Groq Server:  {}", "❌ Unreachable".red()),
    }

    let api_key_str = if api_key.is_empty() {
        std::env::var("OPENROUTER_API_KEY")
            .or_else(|_| std::env::var("GEMINI_API_KEY"))
            .or_else(|_| std::env::var("GROQ_API_KEY"))
            .unwrap_or_default()
    } else { api_key.to_string() };
    let api_key = api_key_str.as_str();
    let key_preview = if api_key.len() > 8 {
        format!("{}...", &api_key[..8])
    } else {
        "???".to_string()
    };
    println!("🔑 API Key:      {} ({})", "✅ Set".green(), key_preview);

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} 🤖 Model:        Testing response...")
            .unwrap(),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    let mut llm = llm_engine::LlmEngine::from_env();
    let test_msg = types::Message::user("Reply with exactly: PONG".to_string());
    match llm.call(&[test_msg]).await {
        Ok(resp) => {
            pb.finish_and_clear();
            if !resp.is_empty() {
                println!("🤖 Model:        {}", "✅ Responding".green());
            } else {
                println!("🤖 Model:        {}", "⚠️  Empty response".yellow());
            }
        }
        Err(e) => {
            pb.finish_and_clear();
            println!("🤖 Model:        {} — {}", "❌ Failed".red(), e);
        }
    }

    let binary = std::env::current_exe().unwrap_or_default();
    println!(
        "⚙️  SEL Binary:   {} ({})",
        "✅ Built".green(),
        binary.display()
    );
    println!();
    Ok(())
}

async fn run_bench(api_key: &str, suite: &str, max_repairs: u8, iterations: u8) -> Result<()> {
    let all_cases: &[(&str, &str, &str)] = &[
        // Python
        ("python", "broken import",    "Create Python file importing from math_utils import add. Create math_utils.py with add(a,b) function. Write pytest test. Run tests."),
        ("python", "wrong assertion",  "Create Python function double(x) returning x*2. Write pytest test asserting double(3)==6. Run tests."),
        ("python", "wrong signature",  "Create Python function greet(name) returning f'Hi {name}'. Write pytest test expecting greet('Alice')=='Hi Alice'. Run tests."),
        ("python", "wrong logic",      "Create Python function is_even(n) returning n%2==0. Write pytest test for is_even(4)==True and is_even(3)==False. Run tests."),
        ("python", "type mismatch",    "Create Python function add(a,b) returning a+b for integers. Write pytest test expecting add(2,3)==5. Run tests."),
        ("python", "missing closing",  "Create Python function factorial(n) with base case n==0 returns 1. Write pytest test for factorial(5)==120. Run tests."),
        ("python", "undefined func",   "Create Python module with helper() returning 42. Write pytest test asserting helper()==42. Run tests."),
        ("python", "wrong logic 2",    "Create Python function max_of_three(a,b,c) returning max(a,b,c). Write pytest test. Run tests."),
        ("python", "syntax error",     "Create Python function add(a,b) returning a+b with correct syntax. Write pytest test. Run tests."),
        ("python", "wrong return",     "Create Python function reverse_string(s) returning s[::-1]. Write pytest test expecting reverse_string('hello')=='olleh'. Run tests."),
        ("python", "missing function", "Create Python class Stack with push(item) and pop() methods. Write pytest test. Run tests."),
        ("python", "runtime error",    "Create Python function divide(a,b) returning None if b==0 else a/b. Write pytest tests: test divide(10,2)==5.0 AND divide(10,0)==None (both branches required). Run tests."),
        // Go
        ("go", "go add",       "Create Go package main with Add(a,b int) int. Create go.mod with module gotest and go 1.21. Write _test.go testing Add(2,3)==5 and Add(-1,1)==0. Run go test."),
        ("go", "go fizzbuzz",  "Create Go package main with FizzBuzz(n int) string returning Fizz/Buzz/FizzBuzz/number. Create go.mod module gotest go 1.21. Write _test.go with 4 test cases using only t.Errorf (no fmt import). Run go test."),
        ("go", "go reverse",   "Create Go package main with Reverse(s string) string. Create go.mod module gotest go 1.21. Write _test.go using only t.Errorf (no fmt): test Reverse(\"hello\")=\"olleh\" and Reverse(\"\")=\"\". Run go test."),
        ("go", "go divide",    "Create Go package main with Divide(a,b float64) (float64,error) returning error if b==0. Create go.mod module gotest go 1.21. Write _test.go testing normal and zero cases. Run go test."),
        // Node
        ("node", "node add",        "Create Node.js CommonJS module math.js exporting add(a,b). Create package.json with jest. Write math.test.js testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
        ("node", "node palindrome", "Create Node.js CommonJS module palindrome.js exporting isPalindrome(s). Create package.json with jest. Write test file testing racecar==true and hello==false. Run npm test."),
        ("node", "node factorial",  "Create Node.js CommonJS module factorial.js exporting factorial(n) with base case 0==1. Create package.json with jest. Write test for factorial(5)==120 and factorial(0)==1. Run npm test."),
        ("node", "node filter",     "Create Node.js CommonJS module filter.js exporting filterEven(arr) returning even numbers. Create package.json with jest. Write test with arrays including empty array case. Run npm test."),
        // TypeScript
        ("typescript", "ts add",        "Create TypeScript file math.ts exporting function add(a:number,b:number):number. Create package.json with jest and ts-jest. Create tsconfig.json. Write math.test.ts testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
        ("typescript", "ts palindrome", "Create TypeScript file palindrome.ts exporting function isPalindrome(s:string):boolean. Create package.json with jest and ts-jest. Create tsconfig.json. Write palindrome.test.ts testing racecar===true and hello===false. Run npm test."),
        ("typescript", "ts factorial",  "Create TypeScript file factorial.ts exporting function factorial(n:number):number with base case 0 returns 1. Create package.json with jest and ts-jest. Create tsconfig.json. Write factorial.test.ts testing factorial(5)===120 and factorial(0)===1. Run npm test."),
        ("typescript", "ts stack",      "Create TypeScript file stack.ts exporting class Stack<T> with push(item:T) pop():T|undefined and isEmpty():boolean. Create package.json with jest and ts-jest. Create tsconfig.json. Write stack.test.ts with push/pop/isEmpty tests. Run npm test."),
        // Rust
        ("rust", "rust add",     "Create Rust library crate. Write Cargo.toml with name=rustadd edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32. Write tests module inside lib.rs testing add(2,3)==5 and add(-1,1)==0. Run cargo test."),
        ("rust", "rust fizzbuzz","Create Rust library crate. Write Cargo.toml name=rustfizz edition=2021. Write src/lib.rs with pub fn fizzbuzz(n:u32)->String returning Fizz Buzz FizzBuzz or number. Write tests module with 4 cases. Run cargo test."),
        ("rust", "rust reverse", "Create Rust library crate. Write Cargo.toml name=rustreverse edition=2021. Write src/lib.rs with pub fn reverse(s:&str)->String. Write tests module testing hello->olleh and empty string. Run cargo test."),
        ("rust", "rust stack",   "Create Rust library crate. Write Cargo.toml name=ruststack edition=2021. Write src/lib.rs with pub struct Stack and impl with push pop is_empty. Write tests module. Run cargo test."),
        // Flask / FastAPI
        ("python", "flask hello",   "Create Python Flask app in app.py with GET /hello route returning JSON {\"message\":\"hello world\"}. Create requirements.txt containing only: flask. Write test_app.py using Flask test client: assert response.status_code==200 and response.get_json()[\"message\"]==\"hello world\". Run pytest."),
        ("python", "fastapi route", "Create Python FastAPI app in main.py with GET /hello route returning {\"message\":\"hello\"}. Create requirements.txt containing: fastapi httpx. Write test_main.py using TestClient from fastapi.testclient: assert response.status_code==200 and response.json()[\"message\"]==\"hello\". Run pytest."),
        // Express multi-file
        ("node", "express api",     "Create Node.js Express app in app.js exporting the express app with GET /ping route returning JSON {ok:true}. Create package.json with jest supertest express. Write app.test.js using supertest: assert status 200 and body.ok===true. Run npm test."),
        // TypeScript Express
        ("typescript", "ts express", "Create TypeScript Express app. Write app.ts exporting express app with GET /health route returning JSON {status:\"ok\"}. Create package.json with ts-jest jest typescript express @types/express supertest @types/supertest. Create tsconfig.json. Write app.test.ts using supertest: assert status 200 and body.status===\"ok\". Run npm test."),
        // v7 Feature Checks
        ("v7", "v7_quickfix",    "Create a Python script using the 'requests' library to fetch 'https://httpbin.org/get'. Write a pytest test asserting status_code is 200. Do NOT use pip_install in your execution commands, let the ModuleNotFoundError happen so we test the agent's QuickFix. Run pytest."),
        ("v7", "v7_go_autofix",  "Create Go package main. Write func PrintMessage() that calls fmt.Println(\"Hello\"). STRICT RULE: You must NOT write `import \"fmt\"` anywhere in the file. Leave it missing! Write a test calling the function. Run go test."),
        ("v7", "v7_rust_quotes", "Create Rust library crate with edition 2021. Write pub fn greet() -> &'static str returning 'Hello' (STRICT RULE: you MUST use single quotes around Hello). Write tests module asserting greet() returns it. Run cargo test."),
        ("v7", "v7_unicode",     "Create a Python function that uses a variable named \u{2018}msg\u{2019} and returns \u{201C}smart quotes\u{201D}. Write a pytest test checking its value. Run tests (the agent's sanitize_code should fix these Unicode bounds)."),
    ];

    let cases: Vec<_> = all_cases
        .iter()
        .filter(|(lang, _, _)| suite == "all" || *lang == suite)
        .collect();

    // v5.7: integration suite له دالة منفصلة
    // v7.1: compile suite
    if suite == "compile" {
        return run_compile_bench(max_repairs).await;
    }
    if suite == "integration" {
        return run_integration_bench("", max_repairs).await;
    }

    if cases.is_empty() {
        println!(
            "❌ Unknown suite '{}'. Use: python, go, node, rust, typescript, integration, all",
            suite
        );
        return Ok(());
    }

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Bench v1.8 — suite: {:<14}║", suite);
    println!("╚══════════════════════════════════════════╝\n");

    let total = cases.len();
    let total_runs = total * iterations as usize;
    let mut passed = 0usize;
    let mut total_repairs = 0usize;
    let mut mutation_killed = 0u32;
    let mut mutation_total = 0u32;
    let tmpdir = std::env::temp_dir();

    for iter in 0..iterations {
        if iterations > 1 {
            println!(
                "\n── Iteration {}/{} ──────────────────────────",
                iter + 1,
                iterations
            );
        }
        for (i, (_lang, name, goal)) in cases.iter().enumerate() {
            let workspace = tmpdir.join(format!("sel-bench-{}-{}", iter, i));
            let _ = std::fs::remove_dir_all(&workspace);
            std::fs::create_dir_all(&workspace).ok();
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template(&format!(
                        "{{spinner:.cyan}} 🔬 [{}/{}] {}...",
                        iter + 1,
                        iterations,
                        name
                    ))
                    .unwrap(),
            );
            pb.enable_steady_tick(Duration::from_millis(80));

            let mut agent = crate::agent::Agent::new(
                api_key.to_string(),
                workspace.clone(),
                goal.to_string(),
                max_repairs,
                types::ContextConfig::default(),
            );
            let ok = agent.run().await.is_ok();
            pb.finish_and_clear();

            let repairs = agent.repair_count();
            total_repairs += repairs;
            let ms = agent.mutation_score();
            if ms >= 0.0 {
                mutation_total += 1;
                if ms >= 1.0 {
                    mutation_killed += 1;
                }
            }

            let status = if ok { "✅" } else { "❌" };
            let ms_str = if ms >= 0.0 {
                format!("{:.0}%", ms * 100.0)
            } else {
                "—".to_string()
            };
            println!(
                "   {} {:20} repairs:{} mutation:{}",
                status, name, repairs, ms_str
            );
            if ok {
                passed += 1;
            }
        }
    }

    let success_rate = passed as f64 / total_runs as f64;
    let avg_repairs = total_repairs as f64 / total_runs as f64;
    let mut_score = if mutation_total > 0 {
        mutation_killed as f64 / mutation_total as f64
    } else {
        -1.0
    };
    let quality = if mut_score >= 0.0 {
        success_rate * mut_score
    } else {
        success_rate
    };

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Bench Results                      ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Suite:          {:<23}║", suite);
    println!("║  Iterations:     {:<23}║", iterations);
    println!(
        "║  Passed:         {:<23}║",
        format!("{}/{}", passed, total_runs)
    );
    println!(
        "║  Success Rate:   {:<23}║",
        format!("{:.1}%", success_rate * 100.0)
    );
    println!("║  Avg Repairs:    {:<23}║", format!("{:.1}", avg_repairs));
    println!(
        "║  Mutation Score: {:<23}║",
        if mut_score >= 0.0 {
            format!("{:.0}%", mut_score * 100.0)
        } else {
            "N/A".to_string()
        }
    );
    println!("║  Quality Index:  {:<23}║", format!("{:.2}", quality));
    println!("╚══════════════════════════════════════════╝\n");

    // POST to Observatory
    let model =
        std::env::var("SEL_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string());
    let version = std::env::var("SEL_VERSION").unwrap_or_else(|_| "v1.9".to_string());
    let body = serde_json::json!({
        "version": version,
        "suite": suite,
        "model": model,
        "passed": passed as i64,
        "total": total as i64,
        "success_rate": success_rate,
        "mutation_score": mut_score,
        "avg_repairs": avg_repairs,
        "quality_index": quality,
        "created_at": ""
    });
    let _ = reqwest::Client::new()
        .post("http://localhost:8777/api/bench")
        .json(&body)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;

    Ok(())
}

async fn run_stress(api_key: &str, max_repairs: u8) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent v7.3.0 — Stress Test                     ║");
    println!("╚══════════════════════════════════════════╝\n");

    let cases: &[(&str, &str)] = &[
        ("broken import",    "Create Python file importing from math_utils import add. Create math_utils.py with add(a,b) function. Write pytest test. Run tests."),
        ("wrong assertion",  "Create Python function double(x) returning x*2. Write pytest test asserting double(3)==6. Run tests."),
        ("wrong signature",  "Create Python function greet(name) returning f'Hi {name}'. Write pytest test expecting greet('Alice')=='Hi Alice'. Run tests."),
        ("wrong logic",      "Create Python function is_even(n) returning n%2==0. Write pytest test for is_even(4)==True and is_even(3)==False. Run tests."),
        ("type mismatch",    "Create Python function add(a,b) returning a+b for integers. Write pytest test expecting add(2,3)==5. Run tests."),
        ("missing closing",  "Create Python function factorial(n) with base case n==0 returns 1. Write pytest test for factorial(5)==120. Run tests."),
        ("undefined func",   "Create Python module with helper() returning 42. Write pytest test asserting helper()==42. Run tests."),
        ("wrong logic 2",    "Create Python function max_of_three(a,b,c) returning max(a,b,c). Write pytest test. Run tests."),
        ("syntax error",     "Create Python function add(a,b) returning a+b with correct syntax. Write pytest test. Run tests."),
        ("wrong return",     "Create Python function reverse_string(s) returning s[::-1]. Write pytest test expecting reverse_string('hello')=='olleh'. Run tests."),
        ("missing function", "Create Python class Stack with push(item) and pop() methods. Write pytest test. Run tests."),
        ("runtime error",    "Create Python function divide(a,b) returning None if b==0 else a/b. Write pytest tests: test divide(10,2)==5.0 AND divide(10,0)==None (both branches required). Run tests."),
        // Go cases
        ("go add",           "Create Go package main with Add(a,b int) int. Create go.mod with module gotest and go 1.21. Write _test.go testing Add(2,3)==5 and Add(-1,1)==0. Run go test."),
        ("go fizzbuzz",      "Create Go package main with FizzBuzz(n int) string returning Fizz/Buzz/FizzBuzz/number. Create go.mod module gotest go 1.21. Write _test.go with 4 test cases using only t.Errorf (no fmt import). Run go test."),
        ("go reverse",       "Create Go package main with Reverse(s string) string. Create go.mod module gotest go 1.21. Write _test.go using only t.Errorf (no fmt): test Reverse(\"hello\")=\"olleh\" and Reverse(\"\")=\"\". Run go test."),
        ("go divide",        "Create Go package main with Divide(a,b float64) (float64,error) returning error if b==0. Create go.mod module gotest go 1.21. Write _test.go testing normal and zero cases. Run go test."),
        // Node cases
        ("node add",         "Create Node.js CommonJS module math.js exporting add(a,b). Create package.json with jest. Write math.test.js testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
        ("node palindrome",  "Create Node.js CommonJS module palindrome.js exporting isPalindrome(s). Create package.json with jest. Write test file testing racecar==true and hello==false. Run npm test."),
        ("node factorial",   "Create Node.js CommonJS module factorial.js exporting factorial(n) with base case 0==1. Create package.json with jest. Write test for factorial(5)==120 and factorial(0)==1. Run npm test."),
        ("node filter",      "Create Node.js CommonJS module filter.js exporting filterEven(arr) returning even numbers. Create package.json with jest. Write test with arrays including empty array case. Run npm test."),
        // Rust cases
        ("rust add",         "Create Rust library crate. Write Cargo.toml with name=rustadd edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32. Write tests module inside lib.rs testing add(2,3)==5 and add(-1,1)==0. Run cargo test."),
        ("rust fizzbuzz",    "Create Rust library crate. Write Cargo.toml name=rustfizz edition=2021. Write src/lib.rs with pub fn fizzbuzz(n:u32)->String returning Fizz Buzz FizzBuzz or number. Write tests module with 4 cases. Run cargo test."),
        ("rust reverse",     "Create Rust library crate. Write Cargo.toml name=rustreverse edition=2021. Write src/lib.rs with pub fn reverse(s:&str)->String. Write tests module testing hello->olleh and empty string. Run cargo test."),
        ("rust stack",       "Create Rust library crate. Write Cargo.toml name=ruststack edition=2021. Write src/lib.rs with pub struct Stack and impl with push pop is_empty. Write tests module. Run cargo test."),
    ];

    let total = cases.len();
    let mut passed = 0usize;
    let mut total_repairs = 0usize;
    let tmpdir = std::env::temp_dir();

    for (i, (name, goal)) in cases.iter().enumerate() {
        let workspace = tmpdir.join(format!("sel-stress-{}", i));
        let _ = std::fs::remove_dir_all(&workspace);

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!("{{spinner:.yellow}} ⏳ Running: {}...", name))
                .unwrap(),
        );
        pb.enable_steady_tick(Duration::from_millis(80));

        let mut ag = agent::Agent::new(
            api_key.to_string(),
            workspace.clone(),
            goal.to_string(),
            max_repairs,
            types::ContextConfig::default(),
        );
        let result = ag.run().await;
        pb.finish_and_clear();

        match result {
            Ok(_) => {
                let repairs = ag.repair_count();
                total_repairs += repairs;
                println!("   ✅ {} (repairs: {})", name.green(), repairs);
                passed += 1;
            }
            Err(_) => println!("   ❌ {}", name.red()),
        }
        let _ = std::fs::remove_dir_all(&workspace);
        if i < total - 1 {
            tokio::time::sleep(Duration::from_secs(12)).await;
        }
    }

    let avg = if passed > 0 {
        total_repairs as f64 / passed as f64
    } else {
        0.0
    };
    println!(
        "\n=== Stress Results: {}/{} passed | avg repairs: {:.1} ===\n",
        passed, total, avg
    );
    Ok(())
}

async fn run_integration_bench(api_key: &str, max_repairs: u8) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Integration Bench v5.7             ║");
    println!("║   Phase1: Build → Phase2: Patch          ║");
    println!("╚══════════════════════════════════════════╝\n");

    let cases: &[(&str, &str, &str, &str)] = &[
        (
            "patch + ref_file",
            "Create Rust library crate. Write Cargo.toml with name=rustcalc edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32 returning a+b. Write tests module inside lib.rs testing add(2,3)==5. Run cargo test.",
            "The crate rustcalc already exists in src/lib.rs. Use patch_file to add pub fn multiply(a:i32,b:i32)->i32 returning a*b to src/lib.rs. Do NOT use write_file. Add 2 tests for multiply inside the tests module. Run cargo test.",
            "src/lib.rs"
        ),
        (
            "fix_rust_string_literals",
            "Create Rust library crate. Write Cargo.toml with name=rustgreet edition=2021. Write src/lib.rs with pub fn greet(name:&str)->String returning format!(\"Hello {}\", name). Write tests module testing greet(\"World\")==\"Hello World\". Run cargo test.",
            "The crate rustgreet already exists. Use patch_file to add pub fn farewell()->&'static str to src/lib.rs. The function must return \"BYE\". Add 1 test asserting farewell()==\"BYE\". Run cargo test.",
            "src/lib.rs"
        ),
        (
            "duplicate detection",
            "Create Rust library crate. Write Cargo.toml with name=rustdup edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32 returning a+b. Write tests module testing add(2,3)==5. Run cargo test.",
            "The crate rustdup already exists with add() already defined. Use patch_file to add pub fn subtract(a:i32,b:i32)->i32 returning a-b to src/lib.rs. Do NOT redefine add(). Add 2 tests for subtract only. Run cargo test.",
            "src/lib.rs"
        ),
        (
            "skeleton multi-file",
            "Create Rust library crate with 2 source files. Write Cargo.toml name=rustmulti edition=2021. Write src/lib.rs with: pub mod math; pub use math::add;. Write src/math.rs with pub fn add(a:i32,b:i32)->i32 returning a+b. Write tests/math_test.rs testing add(2,3)==5. Run cargo test.",
            "The crate rustmulti already exists with src/lib.rs and src/math.rs. Use patch_file to add pub fn multiply(a:i32,b:i32)->i32 to src/math.rs ONLY. Do NOT touch src/lib.rs. Add 2 tests in tests/math_test.rs. Run cargo test.",
            "src/math.rs"
        ),
        (
            "flask add route",
            "Create Python Flask app in app.py with GET /hello route returning JSON {\"message\":\"hello\"}. Create requirements.txt with only: flask. Write test_app.py using Flask test client testing GET /hello returns 200 and message==\"hello\". Run pytest.",
            "The Flask app already exists in app.py. Use patch_file to add GET /goodbye route returning JSON {\"message\":\"goodbye\"} to app.py. Add 1 new test in test_app.py for GET /goodbye returns 200. Do NOT modify existing tests. Run pytest.",
            "app.py"
        ),
        (
            "fastapi add endpoint",
            "Create Python FastAPI app in main.py with GET /hello route returning {\"message\":\"hello\"}. Create requirements.txt with: fastapi httpx. Write test_main.py using TestClient from fastapi.testclient testing GET /hello returns 200. Run pytest.",
            "The FastAPI app already exists in main.py. Use patch_file to add GET /bye route returning {\"message\":\"bye\"} to main.py. Add 1 new test in test_main.py for GET /bye. Do NOT modify existing tests. Run pytest.",
            "main.py"
        ),
        (
            "marketing bot patch reddit",
            "Create a Node.js TypeScript Marketing Bot. Architecture: 1. src/database.ts with in-memory CampaignStore class storing {id,platform,url,date}. 2. src/platforms/devto.ts with DevToClient class taking apiKey, having postArticle(title:string,url:string):Promise<string> method using axios (mock-friendly). 3. src/scheduler.ts with Scheduler class that takes a platform client and has schedule(campaign) method. 4. src/index.ts exporting all. Write src/scheduler.test.ts using jest.mock for axios testing schedule() works. Extra deps: axios",
            "The Marketing Bot already exists with src/database.ts src/platforms/devto.ts src/scheduler.ts src/index.ts. Use patch_file or write_file to ADD src/platforms/reddit.ts with RedditClient class taking apiKey, having postLink(title:string,url:string,subreddit:string):Promise<string> method (axios-based). Add src/platforms/reddit.test.ts using jest.mock for axios testing postLink returns a string id. Do NOT modify existing files except src/index.ts to export RedditClient. Run npm test.",
            "src/scheduler.ts"
        ),
    ];

    let total = cases.len();
    let mut passed = 0usize;
    let mut total_repairs = 0usize;
    let mut phase1_passed = 0usize;
    let mut phase2_passed = 0usize;
    let tmpdir = std::env::temp_dir();

    for (i, (name, goal1, goal2, ref_hint)) in cases.iter().enumerate() {
        println!(
            "\n── Test {}/{}: {} ──────────────────────",
            i + 1,
            total,
            name
        );

        // Phase 1: Build
        let workspace = tmpdir.join(format!("sel-integration-{}", i));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).ok();

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!("{{spinner:.cyan}} Phase1 [{}]...", name))
                .unwrap(),
        );
        pb.enable_steady_tick(Duration::from_millis(80));

        let mut agent1 = crate::agent::Agent::new(
            api_key.to_string(),
            workspace.clone(),
            goal1.to_string(),
            max_repairs,
            types::ContextConfig::default(),
        );
        let ok1 = agent1.run().await.is_ok();
        pb.finish_and_clear();

        let repairs1 = agent1.repair_count();
        if ok1 {
            phase1_passed += 1;
            println!("   ✅ Phase1 passed (repairs: {})", repairs1);
        } else {
            println!(
                "   ❌ Phase1 FAILED (repairs: {}) — skipping Phase2",
                repairs1
            );
            total_repairs += repairs1;
            continue;
        }

        // Phase 2: Patch
        let ref_file_path = workspace.join(ref_hint);
        let ctx_config = types::ContextConfig {
            ref_file: if ref_file_path.exists() {
                Some(ref_file_path)
            } else {
                None
            },
            focus_paths: vec!["src/".to_string()],
            ..Default::default()
        };

        let pb2 = ProgressBar::new_spinner();
        pb2.set_style(
            ProgressStyle::default_spinner()
                .template(&format!("{{spinner:.green}} Phase2 [{}]...", name))
                .unwrap(),
        );
        pb2.enable_steady_tick(Duration::from_millis(80));

        let mut agent2 = crate::agent::Agent::new(
            api_key.to_string(),
            workspace.clone(),
            goal2.to_string(),
            max_repairs,
            ctx_config,
        );
        let ok2 = agent2.run().await.is_ok();
        pb2.finish_and_clear();

        let repairs2 = agent2.repair_count();
        total_repairs += repairs1 + repairs2;

        if ok2 {
            phase2_passed += 1;
            passed += 1;
            println!("   ✅ Phase2 passed (repairs: {})", repairs2);
        } else {
            println!("   ❌ Phase2 FAILED (repairs: {})", repairs2);
        }

        let _ = std::fs::remove_dir_all(&workspace);
    }

    let avg_repairs = if total > 0 {
        total_repairs as f64 / total as f64
    } else {
        0.0
    };
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   Integration Bench Results               ║");
    println!("╠══════════════════════════════════════════╣");
    println!(
        "║  Tests:          {:<23}║",
        format!("{} cases x 2 phases", total)
    );
    println!(
        "║  Phase1 passed:  {:<23}║",
        format!("{}/{}", phase1_passed, total)
    );
    println!(
        "║  Phase2 passed:  {:<23}║",
        format!("{}/{}", phase2_passed, total)
    );
    println!(
        "║  Full passed:    {:<23}║",
        format!("{}/{}", passed, total)
    );
    println!("║  Avg Repairs:    {:<23}║", format!("{:.1}", avg_repairs));
    println!("╚══════════════════════════════════════════╝\n");

    Ok(())
}

async fn run_compare(models: &[String], suite: &str, max_repairs: u8) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent v7.3.0 — Model Comparison       ║");
    println!("╚══════════════════════════════════════════╝\n");
    println!("   Models:  {:?}", models);
    println!("   Suite:   {}", suite);
    println!();

    #[derive(Debug)]
    struct ModelResult {
        model: String,
        passed: usize,
        total: usize,
        avg_repairs: f64,
        mut_score: f64,
        quality: f64,
        elapsed_secs: u64,
        retries: u32,
        connection_errors: u32,
        rate_limits: u32,
        timeouts: u32,
    }

    let all_cases: &[(&str, &str, &str)] = &[
        ("python", "broken import",   "Create Python file importing from math_utils import add. Create math_utils.py with add(a,b) function. Write pytest test. Run tests."),
        ("python", "wrong logic",     "Create Python function is_even(n) returning n%2==0. Write pytest test for is_even(4)==True and is_even(3)==False. Run tests."),
        ("python", "wrong return",    "Create Python function reverse_string(s) returning s[::-1]. Write pytest test expecting reverse_string('hello')=='olleh'. Run tests."),
        ("rust",   "rust add",        "Create Rust library crate. Write Cargo.toml with name=rustadd edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32. Write tests module inside lib.rs testing add(2,3)==5 and add(-1,1)==0. Run cargo test."),
        ("go",     "go add",          "Create Go package main with Add(a,b int) int. Create go.mod with module gotest and go 1.21. Write _test.go testing Add(2,3)==5 and Add(-1,1)==0. Run go test."),
        ("node",   "node add",        "Create Node.js CommonJS module math.js exporting add(a,b). Create package.json with jest. Write math.test.js testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
    ];

    let cases: Vec<_> = all_cases
        .iter()
        .filter(|(lang, _, _)| suite == "all" || *lang == suite)
        .collect();

    if cases.is_empty() {
        println!(
            "❌ Unknown suite '{}'. Use: python, go, node, rust, typescript, all",
            suite
        );
        return Ok(());
    }

    let tmpdir = std::env::temp_dir();
    let mut results: Vec<ModelResult> = Vec::new();

    for model_alias in models {
        println!(
            "\n🤖 Testing model: {} ──────────────────────────",
            model_alias
        );

        let model_cfg = crate::llm_engine::ModelConfig::from_alias(model_alias);
        let api_key = std::env::var(&model_cfg.env_key).unwrap_or_else(|_| {
            println!(
                "   ⚠ {} غير موجود — تخطي النموذج {}",
                model_cfg.env_key, model_alias
            );
            String::new()
        });

        if api_key.is_empty() {
            continue;
        }

        let mut passed = 0usize;
        let mut total_repairs = 0usize;
        let mut mutation_killed = 0u32;
        let mut mutation_total = 0u32;
        let mut total_retries = 0usize;
        let mut total_connection_errors = 0usize;
        let mut total_rate_limits = 0usize;
        let mut total_timeouts = 0usize;
        let total = cases.len();
        let start = std::time::Instant::now();

        for (i, (_lang, name, goal)) in cases.iter().enumerate() {
            let workspace = tmpdir.join(format!("sel-cmp-{}-{}", model_alias, i));
            let _ = std::fs::remove_dir_all(&workspace);
            std::fs::create_dir_all(&workspace).expect("failed to create workspace");

            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template(&format!(
                        "{{spinner:.cyan}} [{}/{}] {}...",
                        i + 1,
                        total,
                        name
                    ))
                    .unwrap(),
            );
            pb.enable_steady_tick(Duration::from_millis(80));

            let mut agent = crate::agent::Agent::new_with_model(
                api_key.clone(),
                model_alias.clone(),
                workspace.clone(),
                goal.to_string(),
                max_repairs,
                types::ContextConfig::default(),
            );
            let run_result = agent.run().await;
            let ok = run_result.is_ok();
            pb.finish_and_clear();

            if let Err(ref e) = run_result {
                println!(
                    "   ❌ {:20} FAILED: {}",
                    name,
                    &e.to_string()[..e.to_string().len().min(80)]
                );
                let _ = std::fs::remove_dir_all(&workspace);
                continue;
            }

            let repairs = agent.repair_count();
            total_repairs += repairs;
            // v6.1: تراكم إحصائيات الاتصال
            let cstats = agent.call_stats();
            total_retries += cstats.retries as usize;
            total_connection_errors += cstats.connection_errors as usize;
            total_rate_limits += cstats.rate_limits as usize;
            total_timeouts += cstats.timeouts as usize;
            let ms = agent.mutation_score();
            if ms >= 0.0 {
                mutation_total += 1;
                if ms >= 1.0 {
                    mutation_killed += 1;
                }
            }

            let status = if ok { "✅" } else { "❌" };
            let ms_str = if ms >= 0.0 {
                format!("{:.0}%", ms * 100.0)
            } else {
                "—".to_string()
            };
            println!(
                "   {} {:20} repairs:{} mutation:{}",
                status, name, repairs, ms_str
            );
            if ok {
                passed += 1;
            }
            let _ = std::fs::remove_dir_all(&workspace);
        }

        let elapsed = start.elapsed().as_secs();
        let success_rate = passed as f64 / total as f64;
        let avg_repairs = total_repairs as f64 / total as f64;
        let mut_score = if mutation_total > 0 {
            mutation_killed as f64 / mutation_total as f64
        } else {
            -1.0
        };
        let quality = if mut_score >= 0.0 {
            success_rate * mut_score
        } else {
            success_rate
        };

        results.push(ModelResult {
            model: model_cfg.model_id.clone(),
            passed,
            total,
            avg_repairs,
            mut_score,
            quality,
            elapsed_secs: elapsed,
            retries: total_retries as u32,
            connection_errors: total_connection_errors as u32,
            rate_limits: total_rate_limits as u32,
            timeouts: total_timeouts as u32,
        });
    }

    // ── v6.1: RAS + DTO ──
    let max_time = results.iter().map(|r| r.elapsed_secs).max().unwrap_or(1);
    let mut scores: Vec<evaluator::ModelScore> = results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let raw = evaluator::RawMetrics {
                tests_passed: r.passed as u32,
                tests_total: r.total as u32,
                mutation_score: if r.mut_score >= 0.0 { r.mut_score } else { 0.0 },
                repairs: (r.avg_repairs * r.total as f64).round() as u32,
                retries: r.retries,
                connection_errors: r.connection_errors,
                rate_limits: r.rate_limits,
                timeouts: r.timeouts,
                elapsed_secs: r.elapsed_secs,
            };
            evaluator::ModelScore::from_metrics(&r.model, &format!("run-{}", i), &raw, max_time)
        })
        .collect();

    scores = evaluator::rank_models(scores);
    evaluator::print_comparison_table(&scores);

    if let Some(best) = scores.iter().find(|s| !s.unstable) {
        println!(
            "🏆 أفضل نموذج: {} (Composite: {:.3} | Correct: {:.2} | Reliable: {:.2})\n",
            best.model, best.composite, best.correctness, best.reliability
        );
    } else {
        println!("⚠ جميع النماذج غير مستقرة — لا يوجد فائز\n");
    }

    Ok(())
}

async fn run_plan(api_key: &str,
    workspace: &std::path::Path,
    plan_file: &std::path::Path,
    max_repairs: u8,
) -> Result<()> {
    let content = std::fs::read_to_string(plan_file)
        .map_err(|e| anyhow::anyhow!("Cannot read plan file: {}", e))?;

    // parse lines: "- [ ] goal text" or "- [x] done"
    let tasks: Vec<String> = content
        .lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.starts_with("- [ ]") {
                Some(t[5..].trim().to_string())
            } else if t.starts_with("* [ ]") {
                Some(t[5..].trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())
        .collect();

    if tasks.is_empty() {
        println!("\n❌ No pending tasks found in plan file.");
        println!("   Use format: - [ ] your goal here");
        return Ok(());
    }

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent — Markdown Plan Runner        ║");
    println!("╠══════════════════════════════════════════╣");
    println!(
        "║  Plan:       {:<27}║",
        plan_file.file_name().unwrap_or_default().to_string_lossy()
    );
    println!("║  Tasks:      {:<27}║", tasks.len());
    println!(
        "║  Workspace:  {:<27}║",
        workspace
            .display()
            .to_string()
            .chars()
            .take(27)
            .collect::<String>()
    );
    println!("╚══════════════════════════════════════════╝\n");

    std::fs::create_dir_all(workspace).ok();

    let mut passed = 0usize;
    let mut total_repairs = 0usize;

    for (i, task) in tasks.iter().enumerate() {
        println!(
            "\n── Task {}/{} ─────────────────────────────────",
            i + 1,
            tasks.len()
        );
        println!("   📋 {}", &task.chars().take(80).collect::<String>());

        let pb = indicatif::ProgressBar::new_spinner();
        pb.set_style(
            indicatif::ProgressStyle::default_spinner()
                .template(&format!(
                    "{{spinner:.cyan}} ⚙️  Task [{}/{}]...",
                    i + 1,
                    tasks.len()
                ))
                .unwrap(),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(80));

        let mut ag = agent::Agent::new(
            api_key.to_string(),
            workspace.to_path_buf(),
            task.clone(),
            max_repairs,
            types::ContextConfig::default(),
        );
        let ok = ag.run().await.is_ok();
        pb.finish_and_clear();

        let repairs = ag.repair_count();
        total_repairs += repairs;

        if ok {
            passed += 1;
            println!("   ✅ Passed (repairs: {})", repairs);
        } else {
            println!("   ❌ Failed (repairs: {})", repairs);
        }
    }

    let avg_repairs = if tasks.len() > 0 {
        total_repairs as f64 / tasks.len() as f64
    } else {
        0.0
    };

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   Plan Results                            ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Tasks:      {:<27}║", tasks.len());
    println!(
        "║  Passed:     {:<27}║",
        format!("{}/{}", passed, tasks.len())
    );
    println!("║  Avg Repairs:{:<27}║", format!("{:.1}", avg_repairs));
    println!("╚══════════════════════════════════════════╝\n");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Health => {
            run_health(&crate::llm_engine::LlmEngine::from_env().primary_name()).await?;
        }
        Commands::Bench {
            suite,
            max_repairs,
            iterations,
        } => {
            run_bench("", &suite, max_repairs, iterations).await?;
        }
        Commands::Stress { max_repairs } => {
            run_stress("", max_repairs).await?;
        }
        Commands::Scan { workspace, json } => {
            cmd_scan(&workspace, json);
        }
        Commands::Compare {
            models,
            suite,
            max_repairs,
        } => {
            run_compare(&models, &suite, max_repairs).await?;
        }
        Commands::Plan {
            workspace,
            plan,
            max_repairs,
        } => {
            run_plan("", &workspace, &plan, max_repairs).await?;
        }
        Commands::BenchRealWorld { tier, max_repairs } => {
            crate::bench_realworld::run_bench_realworld("", tier, max_repairs).await?;
        }
        Commands::Run {
            workspace,
            goal,
            max_repairs,
            dry_run,
            ref_file,
            focus,
        } => {
            println!("\n╔══════════════════════════════════════════╗");
            println!("║   SEL Agent v7.3.0 — State Machine Engine   ║");
            println!("╚══════════════════════════════════════════╝");
            println!("\n📋 Goal: \"{}\"", goal);
            // Provider info
            {
                let engine = crate::llm_engine::LlmEngine::from_env();
                engine.print_info();
            }
            println!("   Workspace:   {}", workspace.display());
            println!("   Max repairs: {}", max_repairs);
            if let Some(ref rf) = ref_file {
                println!("   Ref file:    {}", rf.display());
            }
            if !focus.is_empty() {
                println!("   Focus:       {:?}", focus);
            }

            if dry_run {
                println!("   Mode:         🔍 DRY RUN\n");
                let mut llm = llm_engine::LlmEngine::from_env();
                let prompt = format!("Goal: {}\n\nProvide the complete execution plan.", goal);
                match llm.call(&[types::Message::user(prompt)]).await {
                    Ok(response) => match protocol::parse(&response) {
                        Ok(plan) => {
                            println!("📋 Plan preview ({} commands):\n", plan.commands.len());
                            for (i, cmd) in plan.commands.iter().enumerate() {
                                println!("  [{}/{}] {}", i + 1, plan.commands.len(), cmd.label());
                            }
                            println!("\n✅ DRY RUN complete.");
                        }
                        Err(e) => println!("❌ Plan parse error: {}", e),
                    },
                    Err(e) => println!("❌ LLM error: {}", e),
                }
                return Ok(());
            }

            std::fs::create_dir_all(&workspace)?;
            // v5.4: توسيع ~ في مسار ref-file
            let ref_file_expanded = ref_file.as_ref().map(|p| {
                let s = p.to_string_lossy();
                if s.starts_with("~/") {
                    if let Ok(home) = std::env::var("HOME") {
                        return std::path::PathBuf::from(format!("{}/{}", home, &s[2..]));
                    }
                }
                p.clone()
            });
            let ctx_config = types::ContextConfig {
                ref_file: ref_file_expanded,
                focus_paths: focus.clone(),
                ..Default::default()
            };
            let mut ag = agent::Agent::new(String::new(), workspace, goal, max_repairs, ctx_config);
            ag.run().await?;
        }
    }
    Ok(())
}

// ─── scan command (v6.2) ───────────────────────────────────────────────────

fn cmd_scan(workspace: &str, json: bool) {
    use crate::scanner::scan_project;
    use std::path::Path;

    let path = Path::new(workspace);
    if !path.exists() {
        eprintln!("❌ Workspace not found: {}", workspace);
        std::process::exit(1);
    }

    let profile = scan_project(path);

    if json {
        println!("{}", serde_json::to_string_pretty(&profile).unwrap());
        return;
    }

    // human-readable output
    let conf_bar = confidence_bar(profile.confidence);
    println!();
    println!("📁 Project  : {}", profile.project_name);
    println!("🔤 Language : {}", profile.language);
    println!(
        "📦 Manifest : {}",
        profile
            .dependency_file
            .as_ref()
            .map(|p: &std::path::PathBuf| p.display().to_string())
            .unwrap_or_else(|| "—".to_string())
    );
    println!(
        "📍 Entry    : {}",
        if profile.entry_points.is_empty() {
            "—".to_string()
        } else {
            profile
                .entry_points
                .iter()
                .map(|p: &std::path::PathBuf| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!(
        "🧪 Tests    : {} {}",
        if profile.has_tests { "✅" } else { "❌" },
        profile.test_framework.as_deref().unwrap_or("")
    );
    println!(
        "🏗  Build    : {}",
        profile.build_cmd.as_deref().unwrap_or("—")
    );
    println!(
        "✅ Test cmd : {}",
        profile.test_cmd.as_deref().unwrap_or("—")
    );
    println!(
        "🎯 Confid.  : {:.0}%  {}",
        profile.confidence * 100.0,
        conf_bar
    );
    println!();
}

fn confidence_bar(c: f32) -> String {
    let filled = (c * 10.0).round() as usize;
    let empty = 10 - filled.min(10);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

// ══════════════════════════════════════════════════════
// Compile Bench — v7.1
// ══════════════════════════════════════════════════════

async fn run_compile_bench(max_repairs: u8) -> Result<()> {
    use crate::bench_compile::{all_cases, setup_case, check_result};

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Bench — suite: compile (v7.1)      ║");
    println!("╚══════════════════════════════════════════╝\n");

    let cases = all_cases();
    let total = cases.len();
    let mut passed = 0usize;
    let mut results: Vec<(String, String, usize, bool, String)> = Vec::new();
    let tmpdir = std::env::temp_dir();

    for (i, case) in cases.iter().enumerate() {
        let workspace = tmpdir.join(format!("sel-compile-{}", i));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).ok();

        // Setup: كتابة الملفات المكسورة
        setup_case(case.name, &workspace);

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!(
                    "{{spinner:.cyan}} 🔬 [{}/{}] {}...",
                    i + 1, total, case.name
                ))
                .unwrap(),
        );
        pb.enable_steady_tick(Duration::from_millis(80));

        // تشغيل Agent مع goal محدد
        let mut agent = crate::agent::Agent::new(
            String::new(),
            workspace.clone(),
            case.goal.to_string(),
            case.max_repairs.min(max_repairs),
            crate::types::ContextConfig::default(),
        );
        let ok = agent.run().await.is_ok();
        pb.finish_and_clear();

        let repairs = agent.repair_count();
        let mutation = agent.mutation_score();

        // فحص النتيجة
        let check = check_result(case.name, &workspace, ok, repairs, mutation);

        let status = if check.passed { "✅" } else { "❌" };
        let mut notes = Vec::new();
        if check.created_wrong_files { notes.push("wrong_files".to_string()); }
        if !check.mutation_ok { notes.push("mutation_fail".to_string()); }
        if repairs > 2 { notes.push(format!("repairs:{}", repairs)); }
        let note_str = if notes.is_empty() { "ok".to_string() } else { notes.join(", ") };

        println!(
            "   {} {:<28} repairs:{}  {}",
            status, case.name, repairs, note_str
        );

        if check.passed { passed += 1; }
        results.push((
            case.name.to_string(),
            status.to_string(),
            repairs,
            check.passed,
            note_str,
        ));

        // تنظيف
        let _ = std::fs::remove_dir_all(&workspace);
    }

    // النتائج النهائية
    let rate = passed as f64 / total as f64 * 100.0;
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   Compile Bench Results                   ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Passed:  {}/{}  ({:.0}%)                   ║", passed, total, rate);
    println!("╠══════════════════════════════════════════╣");

    for (name, status, repairs, _, note) in &results {
        println!("║  {} {:<22} r:{} {}",
            status, name, repairs,
            if note.len() > 15 { &note[..15] } else { note }
        );
    }

    println!("╚══════════════════════════════════════════╝");

    if rate >= 100.0 {
        println!("\n   🏆 v7.1 مستقر تماماً");
    } else if rate >= 75.0 {
        println!("\n   ⚠️  بعض القدرات تحتاج تحسين");
    } else {
        println!("\n   ❌ v7.1 يحتاج مراجعة جدية");
    }

    Ok(())
}

```

## File: main_v14_backup.rs

```rust
mod context;
// src/main.rs — v1.3
mod types;
mod protocol;
mod executor;
mod llm;
mod agent;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
#[command(name = "sel-agent", version = "1.3.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(long)] workspace:   PathBuf,
        #[arg(long)] goal:        String,
        #[arg(long, default_value = "3")] max_repairs: u8,
        #[arg(long, default_value = "false")] dry_run: bool,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run { workspace, goal, max_repairs, dry_run } => {
            println!("\n╔══════════════════════════════════════════╗");
            println!("║   SEL Agent v1.3 — State Machine Engine  ║");
            println!("╚══════════════════════════════════════════╝");
            println!("\n📋 Goal: \"{}\"", goal);
            println!("   Workspace:   {}", workspace.display());
            println!("   Max repairs: {}", max_repairs);
            // ── Dry Run v1.3 ──────────────────────────────
            if dry_run {
                println!("   Mode:         🔍 DRY RUN (preview only — nothing will execute)\n");
                let api_key = std::env::var("GROQ_API_KEY")
                    .expect("GROQ_API_KEY not set");
                let llm = llm::LlmClient::new(api_key);
                let prompt = format!("Goal: {}\n\nProvide the complete execution plan.", goal);
                match llm.call(&[types::Message::user(prompt)]).await {
                    Ok(response) => match protocol::parse(&response) {
                        Ok(plan) => {
                            println!("📋 Plan preview ({} commands):\n", plan.commands.len());
                            for (i, cmd) in plan.commands.iter().enumerate() {
                                println!("  [{}/{}] {}", i + 1, plan.commands.len(), cmd.label());
                            }
                            println!("\n✅ DRY RUN complete — nothing was executed.");
                        }
                        Err(e) => println!("❌ Plan parse error: {}", e),
                    },
                    Err(e) => println!("❌ LLM error: {}", e),
                }
                return Ok(());
            }
            // ── Normal Run ────────────────────────────────
            let api_key = std::env::var("GROQ_API_KEY")
                .expect("GROQ_API_KEY not set");
            std::fs::create_dir_all(&workspace)?;
            let mut ag = agent::Agent::new(api_key, workspace, goal, max_repairs);
            ag.run().await?;
        }
    }
    Ok(())
}

```

## File: manifest.rs

```rust
// src/manifest.rs — v7.3: Project Manifest
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub language: String,
    pub files:    Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path:    String,
    pub kind:    FileKind,
    pub exports: Vec<String>,
    pub imports: Vec<String>, // خفيف — أسماء فقط
    pub size:    usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileKind { Source, Test, Config }

impl ProjectManifest {
    pub fn generate(workspace: &Path) -> Self {
        let profile  = crate::scanner::scan_project(workspace);
        let language = format!("{}", profile.language);
        let files    = Self::scan_files(workspace);
        Self { language, files }
    }

    fn scan_files(workspace: &Path) -> Vec<FileEntry> {
        let supported = ["ts","js","py","go","rs","toml","json"];
        let mut entries: Vec<FileEntry> = walkdir::WalkDir::new(workspace)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter(|e| {
                let ext = e.path().extension()
                    .and_then(|s| s.to_str()).unwrap_or("");
                supported.contains(&ext)
            })
            .filter(|e| {
                let s = e.path().to_string_lossy();
                !s.contains("node_modules")
                    && !s.contains("/venv/")
                    && !s.contains("/dist/")
                    && !s.contains("/target/")
                    && !s.contains("/.")
                    && !s.contains("package-lock")
            })
            .filter_map(|e| Self::analyze_file(workspace, e.path()))
            .collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        entries
    }

    fn analyze_file(workspace: &Path, path: &Path) -> Option<FileEntry> {
        let rel  = path.strip_prefix(workspace).ok()?
                       .to_string_lossy().to_string();
        let content = fs::read_to_string(path).ok()?;
        let size    = content.lines().count();
        let ext     = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        let kind = if rel.contains(".test.") || rel.contains("_test.")
                      || rel.starts_with("test_") || rel.contains("/test_") {
            FileKind::Test
        } else if matches!(ext, "json" | "toml") {
            FileKind::Config
        } else {
            FileKind::Source
        };

        let exports = if kind == FileKind::Source {
            Self::extract_exports(&content, ext)
        } else { vec![] };

        let imports = Self::extract_imports(&content, ext);

        Some(FileEntry { path: rel, kind, exports, imports, size })
    }

    // ─── Exports ───────────────────────────────────────────
    fn extract_exports(content: &str, ext: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in content.lines() {
            let t = line.trim();
            match ext {
                "ts" | "js" => {
                    if let Some(rest) = t.strip_prefix("export ") {
                        if let Some(n) = Self::first_ident(rest) {
                            // تجاهل keywords
                            if !matches!(n.as_str(), "default"|"type"|"interface"|"{") {
                                out.push(n);
                            }
                        }
                    }
                }
                "py" => {
                    // دوال وكلاسات على مستوى أعلى (بدون indent)
                    if !line.starts_with(' ') && !line.starts_with('\t') {
                        if t.starts_with("def ") || t.starts_with("class ") {
                            if let Some(n) = Self::ident_after_keyword(t) {
                                if !n.starts_with('_') { out.push(n); }
                            }
                        }
                    }
                }
                "rs" => {
                    if t.starts_with("pub fn ")
                        || t.starts_with("pub struct ")
                        || t.starts_with("pub enum ")
                        || t.starts_with("pub trait ") {
                        if let Some(n) = Self::ident_after_pub(t) { out.push(n); }
                    }
                }
                "go" => {
                    if t.starts_with("func ") || t.starts_with("type ") {
                        if let Some(n) = Self::extract_go_export(t) { out.push(n); }
                    }
                }
                _ => {}
            }
        }
        out.dedup();
        out
    }

    // ─── Imports (خفيف) ─────────────────────────────────────
    fn extract_imports(content: &str, ext: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in content.lines() {
            let t = line.trim();
            match ext {
                "ts" | "js" => {
                    // import ... from "./database"
                    if t.starts_with("import ") {
                        if let Some(from) = t.rfind("from ") {
                            let src = t[from+5..].trim().trim_matches(|c| c=='\''||c=='"'||c==';');
                            if !src.is_empty() { out.push(src.to_string()); }
                        }
                    }
                }
                "py" => {
                    // from .database import X  or  import os
                    if t.starts_with("from ") || t.starts_with("import ") {
                        let parts: Vec<&str> = t.split_whitespace().collect();
                        if parts.len() >= 2 {
                            out.push(parts[1].trim_end_matches(',').to_string());
                        }
                    }
                }
                "go" => {
                    // import "fmt"
                    if t.starts_with('"') && t.ends_with('"') {
                        out.push(t.trim_matches('"').to_string());
                    }
                }
                "rs" => {
                    // use crate::X;  use std::...
                    if t.starts_with("use ") {
                        let src = t[4..].trim_end_matches(';')
                            .split("::").next().unwrap_or("").to_string();
                        if !src.is_empty() { out.push(src); }
                    }
                }
                _ => {}
            }
        }
        out.dedup();
        out
    }

    // ─── Helpers ────────────────────────────────────────────
    fn first_ident(s: &str) -> Option<String> {
        // "function add(" → "add"
        // "class User"    → "User"
        // "const PI"      → "PI"
        let words: Vec<&str> = s.split_whitespace().collect();
        let start = if matches!(words.first(), Some(&"function")|Some(&"class")
                                |Some(&"const")|Some(&"let")|Some(&"var")
                                |Some(&"async")) { 1 } else { 0 };
        words.get(start).map(|w| {
            w.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_')
             .to_string()
        })
    }

    fn ident_after_keyword(line: &str) -> Option<String> {
        // "def add():" → "add"
        // "def get_all(store):" → "get_all"
        line.split_whitespace().nth(1)
            .map(|w| {
                // قطع عند أول ( أو : أو )
                let end = w.find(|c: char| c == '(' || c == ':' || c == ')')
                    .unwrap_or(w.len());
                w[..end].to_string()
            })
    }

    fn ident_after_pub(line: &str) -> Option<String> {
        // "pub fn add(" → "add"
        // "pub struct User" → "User"
        line.split_whitespace().nth(2)
            .map(|w| w.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_').to_string())
    }

    fn extract_go_export(line: &str) -> Option<String> {
        // "func Add(" → "Add"  (capital = exported)
        // "type User struct" → "User"
        let parts: Vec<&str> = line.split_whitespace().collect();
        let name = parts.get(1)?
            .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if name.chars().next()?.is_uppercase() {
            Some(name.to_string())
        } else {
            None
        }
    }

    // ─── Summary للـ LLM ────────────────────────────────────
    pub fn to_summary(&self) -> String {
        if self.files.is_empty() { return String::new(); }

        let mut s = String::from("PROJECT MANIFEST:\n");
        s.push_str(&format!("  Language: {}\n", self.language));
        s.push_str("  Files:\n");

        for f in &self.files {
            let k = match f.kind {
                FileKind::Source => "src",
                FileKind::Test   => "test",
                FileKind::Config => "cfg",
            };
            s.push_str(&format!("    [{k}] {} ({} lines)", f.path, f.size));
            if !f.exports.is_empty() {
                s.push_str(&format!(" exports:[{}]", f.exports.join(",")));
            }
            if !f.imports.is_empty() {
                s.push_str(&format!(" imports:[{}]", f.imports.join(",")));
            }
            s.push('\n');
        }
        s
    }
}

```

## File: memory.rs

```rust
// src/memory.rs — v7.2.0: Error Fingerprinting
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MEMORY_FILE: &str = ".sel_memory.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub failure_kind:    String,
    pub error_signature: String,
    pub normalized_hash: u64,
    pub successful_fix:  String,
    pub count:           u32,
    pub last_seen:       String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FailureMemory {
    pub entries: Vec<MemoryEntry>,
}

/// تطبيع نص الخطأ — يجرّد التفاصيل المتغيرة
pub fn normalize_error(error: &str) -> String {
    let first_line = error.lines().next().unwrap_or(error);
    let s = first_line.to_lowercase();

    // استبدل أرقام بـ N
    let mut out = String::new();
    let mut in_num = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !in_num {
                out.push('N');
                in_num = true;
            }
        } else {
            in_num = false;
            // استبدل single quotes بـ Q
            if c == '\'' || c == '`' {
                out.push('Q');
            } else {
                out.push(c);
            }
        }
    }

    out.trim().to_string()
}

/// تجزئة FNV للنص المطبَّع
pub fn hash_normalized(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |acc, b| {
        acc.wrapping_mul(0x100000001b3).wrapping_add(b as u64)
    })
}

// ─── QuickFix ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum QuickFix {
    InstallPackage { command: String },
    AddGoImport    { symbol:  String },
}

/// إصلاح فوري بدون LLM
pub fn quick_fix(error: &str) -> Option<QuickFix> {
    let lower = error.to_lowercase();

    // Python: ModuleNotFoundError
    if lower.contains("modulenotfounderror") || lower.contains("no module named") {
        let module = extract_module_name(error)?;
        let pkg = match module.as_str() {
            "fastapi"    => "fastapi uvicorn",
            "uvicorn"    => "uvicorn[standard]",
            "sqlalchemy" => "sqlalchemy",
            "pydantic"   => "pydantic",
            "jose"       => "python-jose",
            "passlib"    => "passlib bcrypt",
            "dotenv"     => "python-dotenv",
            "httpx"      => "httpx",
            "pytest"     => "pytest",
            "requests"   => "requests",
            "flask"      => "flask",
            _            => return None,
        };
        return Some(QuickFix::InstallPackage {
            command: format!("pip install {}", pkg),
        });
    }

    // Go: undefined: fmt
    if lower.contains("undefined:") {
        let sym = extract_go_undefined(error)?;
        let go_std = [
            "fmt","errors","strings","strconv","sort",
            "math","os","io","log","time","sync",
            "context","bytes","bufio",
        ];
        if go_std.contains(&sym.as_str()) {
            return Some(QuickFix::AddGoImport { symbol: sym });
        }
    }

    None
}

fn extract_module_name(error: &str) -> Option<String> {
    let lower = error.to_lowercase();
    for prefix in &["no module named '", "no module named \""] {
        if let Some(idx) = lower.find(prefix) {
            let after = &error[idx + prefix.len()..];
            let name = after.split(['.', '\'', '"']).next()?.trim();
            if !name.is_empty() {
                return Some(name.to_lowercase());
            }
        }
    }
    None
}

fn extract_go_undefined(error: &str) -> Option<String> {
    for line in error.lines() {
        if line.contains("undefined:") {
            let after = line.split("undefined:").nth(1)?;
            let sym = after.split_whitespace().next()?.trim();
            if !sym.is_empty() {
                return Some(sym.to_string());
            }
        }
    }
    None
}

// ─── FailureMemory ───────────────────────────────────────────────────────────

fn memory_path() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(MEMORY_FILE)
    } else {
        PathBuf::from(MEMORY_FILE)
    }
}

fn today_str() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("day-{}", secs / 86400)
}

impl FailureMemory {
    pub fn load() -> Self {
        let path = memory_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let path = memory_path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    /// حفظ repair ناجح — يستخدم normalized_hash للتجميع
    pub fn record_success(
        &mut self,
        failure_kind: &str,
        error_sig: &str,
        fix_summary: &str,
    ) {
        let normalized = normalize_error(error_sig);
        let hash = hash_normalized(&normalized);

        if let Some(entry) = self.entries.iter_mut()
            .find(|e| e.normalized_hash == hash)
        {
            entry.count += 1;
            entry.last_seen = today_str();
            entry.successful_fix = fix_summary.chars().take(200).collect();
        } else {
            self.entries.push(MemoryEntry {
                failure_kind:    failure_kind.to_string(),
                error_signature: normalized,
                normalized_hash: hash,
                successful_fix:  fix_summary.chars().take(200).collect(),
                count:           1,
                last_seen:       today_str(),
            });
        }

        if self.entries.len() > 100 {
            self.entries.sort_by(|a, b| b.count.cmp(&a.count));
            self.entries.truncate(100);
        }
        self.save();
    }

    /// جلب hints بالـ normalized_hash
    pub fn get_hints(&self, failure_kind: &str, error_sig: &str) -> String {
        let hash = hash_normalized(&normalize_error(error_sig));

        let relevant: Vec<&MemoryEntry> = self.entries.iter()
            .filter(|e| e.normalized_hash == hash || e.failure_kind == failure_kind)
            .collect();

        if relevant.is_empty() {
            return String::new();
        }

        let mut hints = String::from("\n\nFAILURE MEMORY:\n");
        for entry in relevant.iter().take(3) {
            hints.push_str(&format!(
                "- [{}x] {}: {}\n",
                entry.count, entry.failure_kind, entry.successful_fix
            ));
        }
        hints
    }
}

```

## File: protocol.rs

```rust
// src/protocol.rs — v1.3: JSON Protocol

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════
// Schema
// ══════════════════════════════════════════════════════

#[derive(Debug, Deserialize, Serialize)]
pub struct Plan {
    pub version: String,
    pub commands: Vec<Cmd>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Cmd {
    Run {
        command: String,
    },
    WriteFile {
        path: String,
        content: String,
    },
    AppendFile {
        path: String,
        content: String,
    },
    DeleteFile {
        path: String,
    },
    PatchFile {
        path: String,
        #[serde(default)]
        search: String,
        replace: String,
    },
    ReadFile {
        path: String,
    },
    Mkdir {
        path: String,
    },
    RunTests {
        target: String,
    },
    Done {
        #[serde(default)]
        message: String,
    },
}

impl Cmd {
    pub fn label(&self) -> String {
        match self {
            Cmd::Run { command } => format!("run: {}", &command[..command.len().min(60)]),
            Cmd::WriteFile { path, .. } => format!("write_file: {}", path),
            Cmd::AppendFile { path, .. } => format!("append_file: {}", path),
            Cmd::DeleteFile { path } => format!("delete_file: {}", path),
            Cmd::PatchFile { path, .. } => format!("patch_file: {}", path),
            Cmd::ReadFile { path } => format!("read_file: {}", path),
            Cmd::Mkdir { path } => format!("mkdir: {}", path),
            Cmd::RunTests { target } => format!("run_tests: {}", target),
            Cmd::Done { message } => format!("done: {}", message),
        }
    }
    pub fn hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        match self {
            Cmd::Run { command } => {
                "run".hash(&mut h);
                command.hash(&mut h);
            }
            Cmd::WriteFile { path, content } => {
                "write_file".hash(&mut h);
                path.hash(&mut h);
                content.hash(&mut h);
            }
            Cmd::AppendFile { path, content } => {
                "append_file".hash(&mut h);
                path.hash(&mut h);
                content.hash(&mut h);
            }
            Cmd::DeleteFile { path } => {
                "delete_file".hash(&mut h);
                path.hash(&mut h);
            }
            Cmd::PatchFile {
                path,
                search,
                replace,
            } => {
                "patch_file".hash(&mut h);
                path.hash(&mut h);
                search.hash(&mut h);
                replace.hash(&mut h);
            }
            Cmd::ReadFile { path } => {
                "read_file".hash(&mut h);
                path.hash(&mut h);
            }
            Cmd::Mkdir { path } => {
                "mkdir".hash(&mut h);
                path.hash(&mut h);
            }
            Cmd::RunTests { target } => {
                "run_tests".hash(&mut h);
                target.hash(&mut h);
            }
            Cmd::Done { .. } => {
                "done".hash(&mut h);
            }
        }
        format!("{:x}", h.finish())
    }
    pub fn is_done(&self) -> bool {
        matches!(self, Cmd::Done { .. })
    }
    pub fn is_run_tests(&self) -> bool {
        matches!(self, Cmd::RunTests { .. })
    }
    pub fn is_write_file(&self) -> bool {
        matches!(self, Cmd::WriteFile { .. } | Cmd::AppendFile { .. })
    }
    pub fn is_delete_file(&self) -> bool {
        matches!(self, Cmd::DeleteFile { .. })
    }
    pub fn is_patch_file(&self) -> bool {
        matches!(self, Cmd::PatchFile { .. })
    }
}

// ══════════════════════════════════════════════════════
// Parser
// ══════════════════════════════════════════════════════

pub fn extract_json(text: &str) -> Option<&str> {
    let marker = "```json";
    if let Some(s) = text.find(marker) {
        let rest = text[s + marker.len()..].trim_start_matches('\n');
        if let Some(e) = rest.find("```") {
            return Some(rest[..e].trim());
        }
    }
    // Fallback: try finding outermost brackets if markdown tags are omitted
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                return Some(&text[start..=end]);
            }
        }
    }
    None
}

/// يصلح escape sequences غير الصالحة في JSON التي يولدها LLM
/// مثال: \' → ' و \` → `
fn fix_json_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 64);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut in_string = false;

    while i < chars.len() {
        let c = chars[i];

        if in_string {
            if c == '\\' && i + 1 < chars.len() {
                let next = chars[i + 1];
                match next {
                    // valid JSON escapes — keep both chars
                    '"' | '\\' | '/' | 'n' | 'r' | 't' | 'b' | 'f' | 'u' => {
                        result.push('\\');
                        result.push(next);
                        i += 2;
                        continue;
                    }
                    // invalid escape — drop backslash, keep char only
                    _ => {
                        result.push(next);
                        i += 2;
                        continue;
                    }
                }
            } else if c == '\n' {
                result.push('\\');
                result.push('n');
            } else if c == '\r' {
                result.push('\\');
                result.push('r');
            } else if c == '\t' {
                result.push('\\');
                result.push('t');
            } else {
                if c == '"' {
                    in_string = false;
                }
                result.push(c);
            }
        } else {
            if c == '"' {
                in_string = true;
            }
            result.push(c);
        }

        i += 1;
    }
    result
}

pub fn parse(response: &str) -> Result<Plan> {
    let json =
        extract_json(response).ok_or_else(|| anyhow!("No ```json block found in response"))?;
    let cleaned = fix_json_escapes(json);
    serde_json::from_str(&cleaned).map_err(|e| {
        anyhow!("JSON parse error: {}\n---\n{}", e, {
            let start = json.len().min(600).saturating_sub(50);
            let end = json.len().min(800);
            &json[start..end]
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plan() {
        let r = r#"
Some text
```json
{"version":"1.0","commands":[
  {"type":"run","command":"echo hi"},
  {"type":"write_file","path":"a.py","content":"x=1"},
  {"type":"run_tests","target":"a.py"},
  {"type":"done","message":"ok"}
]}
```"#;
        let p = parse(r).unwrap();
        assert_eq!(p.commands.len(), 4);
        assert!(p.commands[0].label().contains("echo hi"));
        assert!(p.commands.last().unwrap().is_done());
    }

    #[test]
    fn fails_without_block() {
        assert!(parse("no json here").is_err());
    }
}

/// Ensures test files are written before RunTests is called.
pub fn validate_test_order(plan: &Plan) -> Result<(), String> {
    let has_run_tests = plan
        .commands
        .iter()
        .any(|c| matches!(c, Cmd::RunTests { .. }));
    if !has_run_tests {
        return Ok(());
    }

    let test_pos = plan.commands.iter().position(|c| match c {
        Cmd::WriteFile { path, .. } => path.contains("test"),
        _ => false,
    });
    let run_pos = plan
        .commands
        .iter()
        .position(|c| matches!(c, Cmd::RunTests { .. }));

    match (test_pos, run_pos) {
        (Some(t), Some(r)) if t < r => Ok(()),
        (None, _) => Err("Plan calls RunTests but writes no test file".into()),
        _ => Err("Test file must be written before RunTests".into()),
    }
}

```

## File: scaffold_engine.rs

```rust
// scaffold_engine.rs — v6.3
// Phase 1: يُجهّز البيئة قبل LLM — حتمي 100%
// Pipeline: ScaffoldEngine::prepare() → LLM::plan_logic_only() → Executor::run()

use crate::goal_parser;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectKind {
    TypeScript,
    Python,
    Rust,
    Go,
    Unknown,
}

#[derive(Debug)]
pub struct ScaffoldResult {
    pub kind: ProjectKind,
    pub ready: bool,
    pub logic_hint: String, // يُرسَل للـ LLM بدلاً من تعليمات البيئة
    pub files_created: Vec<String>,
}

// ─── Pinned Stacks ────────────────────────────────────────
const TS_JEST_DEPS: &str = "typescript@5.3.3 ts-jest@29.1.1 jest@29.7.0 @types/jest@29.5.11";

const PACKAGE_JSON_TS: &str = r#"{
  "name": "sel-project",
  "version": "1.0.0",
  "scripts": {
    "test": "jest",
    "build": "tsc"
  },
  "jest": {
    "preset": "ts-jest",
    "testEnvironment": "node",
    "testMatch": ["**/*.test.ts"]
  },
  "devDependencies": {
    "typescript": "5.3.3",
    "ts-jest": "29.1.1",
    "jest": "29.7.0",
    "@types/jest": "29.5.11"
  }
}"#;

const TSCONFIG_JSON: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "lib": ["ES2020"],
    "strict": true,
    "esModuleInterop": true,
    "outDir": "./dist",
    "rootDir": "./"
  },
  "include": ["**/*.ts"],
  "exclude": ["node_modules", "dist"]
}"#;

// ─── نقطة الدخول ──────────────────────────────────────────
pub async fn prepare(workspace: &Path, goal: &str) -> ScaffoldResult {
    let parsed = goal_parser::parse(workspace, goal);
    let kind = parsed.kind.clone();

    match &kind {
        ProjectKind::TypeScript => scaffold_typescript(workspace, &parsed.extra_deps).await,
        ProjectKind::Python => scaffold_python(workspace, &parsed.extra_deps).await,
        ProjectKind::Unknown => ScaffoldResult {
            kind,
            ready: false,
            logic_hint: String::new(),
            files_created: vec![],
        },
        _ => ScaffoldResult {
            kind,
            ready: false,
            logic_hint: String::new(),
            files_created: vec![],
        },
    }
}

// detect_kind moved to goal_parser.rs — v6.5

// ─── Scaffold TypeScript ──────────────────────────────────
async fn scaffold_typescript(workspace: &Path, extra_deps: &[String]) -> ScaffoldResult {
    println!("   🏗  Scaffold: TypeScript environment");
    let mut created = vec![];

    // 1) package.json — ثابت دائماً
    let pkg_path = workspace.join("package.json");
    let pkg_content = if pkg_path.exists() {
        // صحّح الموجود بدلاً من الكتابة فوقه
        normalize_existing_package_json(&pkg_path)
    } else {
        PACKAGE_JSON_TS.to_string()
    };
    std::fs::write(&pkg_path, &pkg_content).ok();
    println!("   ✅ package.json ready (pinned TS stack)");
    created.push("package.json".to_string());

    // 2) tsconfig.json
    let ts_path = workspace.join("tsconfig.json");
    if !ts_path.exists() {
        std::fs::write(&ts_path, TSCONFIG_JSON).ok();
        println!("   ✅ tsconfig.json ready");
        created.push("tsconfig.json".to_string());
    } else {
        println!("   ⏭  tsconfig.json exists — skip");
    }

    // 3) احذف jest.config.js إذا وُجد (يسبب conflict)
    let jest_cfg = workspace.join("jest.config.js");
    let jest_cfg_ts = workspace.join("jest.config.ts");
    for cfg in [&jest_cfg, &jest_cfg_ts] {
        if cfg.exists() {
            std::fs::remove_file(cfg).ok();
            println!(
                "   🗑  Removed {:?} — config in package.json only",
                cfg.file_name().unwrap_or_default()
            );
        }
    }

    // 4) npm install بالـ pinned stack
    let node_modules = workspace.join("node_modules");
    if !node_modules.exists() {
        println!("   📦 Installing pinned TS stack...");
        let mut npm_args: Vec<&str> = vec!["install", "--save-dev"];
        let ts_deps: Vec<&str> = TS_JEST_DEPS.split_whitespace().collect();
        npm_args.extend_from_slice(&ts_deps);
        let extra_refs: Vec<&str> = extra_deps.iter().map(|s| s.as_str()).collect();
        npm_args.extend_from_slice(&extra_refs);
        if !extra_deps.is_empty() {
            println!("   📦 Extra deps: {}", extra_deps.join(", "));
        }
        let out = tokio::process::Command::new("npm")
            .args(&npm_args)
            .current_dir(workspace)
            .output()
            .await;

        match out {
            Ok(o) if o.status.success() => {
                println!("   ✅ Dependencies installed (pinned)");
                created.push("node_modules".to_string());
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                eprintln!("   ❌ TypeScript Scaffold FATAL: npm install failed with status {}", o.status);
                eprintln!("   💡 Details: {}", &err[..err.len().min(200)]);
                return ScaffoldResult {
                    kind: ProjectKind::TypeScript,
                    ready: false,
                    logic_hint: format!("npm install failed: {}", err),
                    files_created: created,
                };
            }
            Err(e) => {
                eprintln!("   ❌ TypeScript Scaffold FATAL: npm install failed: {}", e);
                eprintln!("   💡 Fix: ensure node/npm are installed and workspace is writable");
                return ScaffoldResult {
                    kind: ProjectKind::TypeScript,
                    ready: false,
                    logic_hint: format!("npm install error: {}", e),
                    files_created: created,
                };
            }
        }
    } else {
        println!("   ⏭  node_modules exists — skip install");
    }

    ScaffoldResult {
        kind: ProjectKind::TypeScript,
        ready: true,
        logic_hint: build_ts_logic_hint(workspace),
        files_created: created,
    }
}

// ─── Scaffold Python ──────────────────────────────────────
async fn scaffold_python(workspace: &Path, extra_deps: &[String]) -> ScaffoldResult {
    println!("   🏗  Scaffold: Python environment");
    let mut created = vec![];

    // 1) venv
    let venv = workspace.join("venv");
    if !venv.exists() {
        println!("   📦 Creating venv...");
        let out = tokio::process::Command::new("python3")
            .args(["-m", "venv", "venv"])
            .current_dir(workspace)
            .output()
            .await;

        if out.map(|o| o.status.success()).unwrap_or(false) {
            println!("   ✅ venv created");
            created.push("venv".to_string());

            // 2) تثبيت pytest مباشرة بعد إنشاء venv
            let _ = tokio::process::Command::new("venv/bin/pip")
                .args(["install", "pytest==8.1.1", "pytest-cov==5.0.0", "-q"])
                .current_dir(workspace)
                .output()
                .await;
            println!("   ✅ pytest installed (pinned)");

            // تثبيت extra_deps من GoalParser
            if !extra_deps.is_empty() {
                println!("   📦 Installing extra deps: {}", extra_deps.join(", "));
                let mut pip_args = vec!["install", "-q"];
                let extra_refs: Vec<&str> = extra_deps.iter().map(|s| s.as_str()).collect();
                pip_args.extend_from_slice(&extra_refs);
                let pip_out = tokio::process::Command::new("venv/bin/pip")
                    .args(&pip_args)
                    .current_dir(workspace)
                    .output()
                    .await;
                match pip_out {
                    Ok(o) if o.status.success() => println!("   ✅ Extra deps installed"),
                    Ok(o) => println!(
                        "   ⚠️  Extra deps warning: {}",
                        String::from_utf8_lossy(&o.stderr)
                            .chars()
                            .take(200)
                            .collect::<String>()
                    ),
                    Err(e) => println!("   ⚠️  Extra deps failed: {}", e),
                }
            }
        }
    } else {
        println!("   ⏭  venv exists — skip");
    }

    ScaffoldResult {
        kind: ProjectKind::Python,
        ready: true,
        logic_hint: build_py_logic_hint(workspace),
        files_created: created,
    }
}

// ─── تصحيح package.json الموجود ───────────────────────────
fn normalize_existing_package_json(path: &Path) -> String {
    let src = std::fs::read_to_string(path).unwrap_or_default();
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&src) {
        // أضف jest config داخل package.json
        v["jest"] = serde_json::json!({
            "preset": "ts-jest",
            "testEnvironment": "node",
            "testMatch": ["**/*.test.ts"]
        });
        // صحّح scripts
        v["scripts"]["test"] = serde_json::json!("jest");
        v["scripts"]["build"] = serde_json::json!("tsc");
        // صحّح devDependencies بالـ pinned stack
        v["devDependencies"] = serde_json::json!({
            "typescript": "5.3.3",
            "ts-jest": "29.1.1",
            "jest": "29.7.0",
            "@types/jest": "29.5.11"
        });
        // احذف "type":"module" — يكسر jest
        if let Some(obj) = v.as_object_mut() {
            obj.remove("type");
        }
        return serde_json::to_string_pretty(&v).unwrap_or(PACKAGE_JSON_TS.to_string());
    }
    PACKAGE_JSON_TS.to_string()
}

// ─── Logic Hints للـ LLM ──────────────────────────────────
fn build_ts_logic_hint(workspace: &Path) -> String {
    let files: Vec<String> = std::fs::read_dir(workspace)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.ends_with(".ts") && !n.ends_with(".test.ts"))
                .collect()
        })
        .unwrap_or_default();

    let existing = if files.is_empty() {
        String::new()
    } else {
        format!("\nExisting TS files: {}", files.join(", "))
    };

    format!(
        "\n[SCAFFOLD READY — TypeScript]\n\
        Environment is fully configured. Do NOT create or modify:\n\
        - package.json (ready with pinned deps + jest config)\n\
        - tsconfig.json (ready)\n\
        - jest.config.js (not needed — config is in package.json)\n\
        - node_modules (installed)\n\
        Your job: write ONLY .ts files (TypeScript). NEVER write .js files.\n\
        Jest testMatch is: **/*.test.ts — .js files will NOT be found by Jest.\n\
        RULE: Every source file must end in .ts, every test file must end in .test.ts\n\
        Test command: npm test{}\n",
        existing
    )
}

fn build_py_logic_hint(workspace: &Path) -> String {
    format!(
        "\n[SCAFFOLD READY — Python]\n\
        Environment is fully configured. Do NOT create venv or install pytest.\n\
        venv is at: {}/venv\n\
        pytest is installed and ready.\n\
        Your job: write ONLY the logic .py files and test files.\n\
        Test command: venv/bin/pytest -v --tb=short\n",
        workspace.display()
    )
}

```

## File: scanner.rs

```rust
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────
// Types
// ─────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Language {
    Rust,
    Python,
    Go,
    Node,
    TypeScript,
    Unknown,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Rust => write!(f, "Rust"),
            Language::Python => write!(f, "Python"),
            Language::Go => write!(f, "Go"),
            Language::Node => write!(f, "Node.js"),
            Language::TypeScript => write!(f, "TypeScript"),
            Language::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectProfile {
    pub project_name: String,
    pub language: Language,
    pub dependency_file: Option<PathBuf>,
    pub entry_points: Vec<PathBuf>,
    pub test_framework: Option<String>,
    pub has_tests: bool,
    pub build_cmd: Option<String>,
    pub test_cmd: Option<String>,
    pub confidence: f32,
}

// ─────────────────────────────────────────
// Main entry point
// ─────────────────────────────────────────

pub fn scan_project(workspace: &Path) -> ProjectProfile {
    let project_name = workspace
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // 1. detect language via manifest (قاطع)
    let (language, dependency_file, base_confidence) = detect_language(workspace);

    // 2. entry points
    let entry_points = detect_entry_points(workspace, &language);
    let entry_bonus: f32 = if !entry_points.is_empty() { 0.2 } else { 0.0 };

    // 3. tests
    let (has_tests, test_framework) = detect_tests(workspace, &language);
    let test_bonus: f32 = if has_tests { 0.2 } else { 0.0 };
    let framework_bonus: f32 = if test_framework.is_some() { 0.1 } else { 0.0 };

    // 4. confidence
    let confidence = (base_confidence + entry_bonus + test_bonus + framework_bonus).min(1.0_f32);

    // 5. commands
    let (build_cmd, test_cmd) = infer_commands(&language, &test_framework);

    ProjectProfile {
        project_name,
        language,
        dependency_file,
        entry_points,
        test_framework,
        has_tests,
        build_cmd,
        test_cmd,
        confidence,
    }
}

// ─────────────────────────────────────────
// Language detection
// returns: (Language, manifest_path, base_confidence)
// ─────────────────────────────────────────

fn detect_language(workspace: &Path) -> (Language, Option<PathBuf>, f32) {
    // manifest قاطع → base 0.5
    let manifests: &[(&str, Language, f32)] = &[
        ("Cargo.toml", Language::Rust, 0.5),
        ("go.mod", Language::Go, 0.5),
        ("package.json", Language::Node, 0.5),
        ("tsconfig.json", Language::TypeScript, 0.5),
    ];

    for (filename, lang, base) in manifests {
        let p = workspace.join(filename);
        if p.exists() {
            // TypeScript: تحقق إضافي من وجود .ts files
            if *lang == Language::TypeScript {
                if has_files_with_ext(workspace, "ts") {
                    return (Language::TypeScript, Some(p), *base);
                }
                // package.json بدون .ts → Node
                continue;
            }
            return (lang.clone(), Some(p), *base);
        }
    }

    // package.json غير موجود لكن .ts موجود
    if has_files_with_ext(workspace, "ts") {
        return (Language::TypeScript, None, 0.4);
    }

    // Python — heuristic (ليس manifest قاطع)
    let py_manifests = [
        "pyproject.toml",
        "setup.py",
        "setup.cfg",
        "requirements.txt",
    ];
    for name in &py_manifests {
        let p = workspace.join(name);
        if p.exists() {
            return (Language::Python, Some(p), 0.5);
        }
    }

    // *.py موجود بدون manifest → base 0.3
    if has_files_with_ext(workspace, "py") {
        return (Language::Python, None, 0.3);
    }

    (Language::Unknown, None, 0.0)
}

// ─────────────────────────────────────────
// Entry points
// ─────────────────────────────────────────

fn detect_entry_points(workspace: &Path, language: &Language) -> Vec<PathBuf> {
    let candidates: &[&str] = match language {
        Language::Rust => &["src/main.rs", "src/lib.rs"],
        Language::Python => &["main.py", "app.py", "__main__.py", "src/main.py"],
        Language::Go => &["main.go", "cmd/main.go"],
        Language::Node => &["index.js", "src/index.js", "app.js"],
        Language::TypeScript => &["index.ts", "src/index.ts", "src/main.ts"],
        Language::Unknown => &[],
    };

    candidates
        .iter()
        .map(|c| workspace.join(c))
        .filter(|p| p.exists())
        .collect()
}

// ─────────────────────────────────────────
// Test detection
// ─────────────────────────────────────────

fn detect_tests(workspace: &Path, language: &Language) -> (bool, Option<String>) {
    match language {
        Language::Rust => {
            // Rust: tests داخل src/ أو مجلد tests/
            let tests_dir = workspace.join("tests");
            let has = tests_dir.exists() || dir_contains_pattern(workspace.join("src"), "#[test]");
            let framework = if has {
                Some("cargo test".to_string())
            } else {
                None
            };
            (has, framework)
        }
        Language::Python => {
            let pytest_ini = ["pytest.ini", "pyproject.toml", "setup.cfg"]
                .iter()
                .any(|f| workspace.join(f).exists());
            let tests_dir = workspace.join("tests").exists() || workspace.join("test").exists();
            let has_test_files =
                has_files_matching(workspace, "test_") || has_files_matching(workspace, "_test.py");

            if pytest_ini || tests_dir || has_test_files {
                let framework = if pytest_ini {
                    Some("pytest".to_string())
                } else {
                    Some("pytest".to_string()) // default لـ Python
                };
                (true, framework)
            } else {
                (false, None)
            }
        }
        Language::Go => {
            let has = has_files_matching(workspace, "_test.go");
            let framework = if has {
                Some("go test".to_string())
            } else {
                None
            };
            (has, framework)
        }
        Language::Node | Language::TypeScript => {
            // تحقق من scripts.test في package.json
            let pkg = workspace.join("package.json");
            if pkg.exists() {
                if let Ok(content) = fs::read_to_string(&pkg) {
                    if content.contains("\"test\"") {
                        let fw = if content.contains("jest") {
                            "jest"
                        } else if content.contains("mocha") {
                            "mocha"
                        } else {
                            "npm test"
                        };
                        return (true, Some(fw.to_string()));
                    }
                }
            }
            (false, None)
        }
        Language::Unknown => (false, None),
    }
}

// ─────────────────────────────────────────
// Command inference
// ─────────────────────────────────────────

fn infer_commands(
    language: &Language,
    test_framework: &Option<String>,
) -> (Option<String>, Option<String>) {
    match language {
        Language::Rust => (
            Some("cargo build --release".to_string()),
            Some("cargo test".to_string()),
        ),
        Language::Python => (
            None,
            test_framework
                .clone()
                .or_else(|| Some("pytest".to_string()))
                .into(),
        ),
        Language::Go => (
            Some("go build ./...".to_string()),
            Some("go test ./...".to_string()),
        ),
        Language::Node => (
            Some("npm install && npm run build".to_string()),
            Some(
                test_framework
                    .clone()
                    .unwrap_or_else(|| "npm test".to_string()),
            ),
        ),
        Language::TypeScript => (
            Some("npm install && npm run build".to_string()),
            Some(
                test_framework
                    .clone()
                    .unwrap_or_else(|| "npm test".to_string()),
            ),
        ),
        Language::Unknown => (None, None),
    }
}

// ─────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────

fn has_files_with_ext(dir: &Path, ext: &str) -> bool {
    fs::read_dir(dir).ok().map_or(false, |entries| {
        entries.filter_map(|e| e.ok()).any(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map_or(false, |x| x == ext)
        })
    })
}

fn has_files_matching(dir: &Path, pattern: &str) -> bool {
    walk_dir(dir, 3).iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .map_or(false, |n| n.contains(pattern))
    })
}

fn dir_contains_pattern(dir: PathBuf, pattern: &str) -> bool {
    walk_dir(&dir, 2)
        .iter()
        .any(|p| fs::read_to_string(p).map_or(false, |content| content.contains(pattern)))
}

/// walk directory up to max_depth, returns all files
fn walk_dir(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if max_depth == 0 || !dir.is_dir() {
        return results;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                results.push(path);
            } else if path.is_dir() {
                // تجاهل مجلدات البناء والـ deps
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !matches!(name, "target" | "node_modules" | ".git" | "__pycache__") {
                    results.extend(walk_dir(&path, max_depth - 1));
                }
            }
        }
    }
    results
}

```

## File: types.rs

```rust
// src/types.rs — v1.3: الأنواع الأساسية

use std::collections::HashSet;
use std::path::PathBuf;

// ══════════════════════════════════════════════════════
// State Machine
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum AgentState {
    Planning,
    Executing,
    Repairing,
    Done,
    Failed(String),
}

// ══════════════════════════════════════════════════════
// سياق التنفيذ — الحالة الفعلية للنظام
// ══════════════════════════════════════════════════════

#[derive(Debug, Default)]
pub struct ExecutionContext {
    pub tests_passed: bool,
    pub last_exit_code: Option<i32>,
    pub failed_steps: Vec<FailedStep>,
    pub repair_attempts: u8,
    pub successful_hashes: HashSet<String>,
    pub max_repairs: u8,
    pub start_time: Option<std::time::Instant>,
    pub mutations_total: u32,
    pub mutations_killed: u32,
    pub replan_attempts: u8,                // v5.6: Unique Patch Enforcer
    pub last_failed_steps: Vec<FailedStep>, // v5.8: نسخة احتياطية قبل المسح
    pub _last_failure_kind: String,         // v5.8: للـ memory
    pub _last_error_sig: String,            // v5.8
    pub current_failure_kind: Option<FailureKind>, // v6.4
    pub skip_mutation: bool, // v6.5: disable mutation enforcement for real-world bench
}

impl ExecutionContext {
    pub fn new(max_repairs: u8) -> Self {
        Self {
            max_repairs,
            start_time: None,
            successful_hashes: std::collections::HashSet::new(),
            ..Default::default()
        }
    }
    pub fn reset_for_repair(&mut self) {
        self.tests_passed = false;
        self.last_exit_code = None;
        if !self.failed_steps.is_empty() {
            self.last_failed_steps = self.failed_steps.clone(); // v5.8
        }
        self.failed_steps.clear();
    }
    pub fn has_failures(&self) -> bool {
        !self.failed_steps.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct FailedStep {
    pub step_index: usize,
    pub label: String,
    pub stderr: String,
    pub exit_code: i32,
    pub culprit_file: Option<String>, // الملف المسؤول عن الخطأ
}

impl FailedStep {
    pub fn extract_culprit(stderr: &str) -> Option<String> {
        for line in stderr.lines() {
            // Python traceback: File "/path/file.py", line 42
            if line.contains("File \"") && line.contains(".py") {
                if let Some(s) = line.find("File \"") {
                    let rest = &line[s + 6..];
                    if let Some(e) = rest.find('"') {
                        let path = &rest[..e];
                        if !path.contains("venv") && !path.contains("site-packages") {
                            let base = path.rfind('/').map(|i| i + 1).unwrap_or(0);
                            let name = &path[base..];
                            if !name.starts_with("test_") {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
            // pytest: service.py:1: in <module>
            if line.contains(".py:") && line.contains(": in ") {
                // تجاهل مسارات stdlib وvenv
                if line.contains("/usr/lib")
                    || line.contains("venv/")
                    || line.contains("site-packages")
                {
                    continue;
                }
                if let Some(pos) = line.find(".py:") {
                    let start = line[..pos]
                        .rfind(|c: char| c == '/' || c == ' ' || c == '\t')
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    let name = &line[start..pos + 3];
                    if !name.starts_with("test_") && !name.starts_with("__") {
                        return Some(name.to_string());
                    }
                }
            }
            // Rust: --> src/lib.rs:42:5
            if line.trim_start().starts_with("--> ") {
                let rest = line.trim_start().trim_start_matches("--> ");
                if let Some(colon) = rest.find(':') {
                    let path = &rest[..colon];
                    let base = path.rfind('/').map(|i| i + 1).unwrap_or(0);
                    return Some(path[base..].to_string());
                }
            }
            // Go: file.go:42
            if line.contains(".go:") {
                if let Some(pos) = line.find(".go:") {
                    let start = line[..pos]
                        .rfind(|c: char| c == '/' || c == ' ')
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    return Some(line[start..pos + 3].to_string());
                }
            }
            // Node.js: file.js:42
            if line.contains(".js:") && !line.contains("node_modules") {
                if let Some(pos) = line.find(".js:") {
                    let start = line[..pos]
                        .rfind(|c: char| c == '/' || c == ' ')
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    return Some(line[start..pos + 3].to_string());
                }
            }
        }
        None
    }
}
// ══════════════════════════════════════════════════════
// نتيجة التنفيذ
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ExecResult {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

impl ExecResult {
    pub fn ok(msg: impl Into<String>) -> Self {
        Self {
            success: true,
            exit_code: 0,
            stdout: msg.into(),
            stderr: String::new(),
            duration_ms: 0,
        }
    }
    pub fn fail(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            exit_code: 1,
            stdout: String::new(),
            stderr: msg.into(),
            duration_ms: 0,
        }
    }
}

// ══════════════════════════════════════════════════════
// Message
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(s: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: s.into(),
        }
    }
    pub fn user(s: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: s.into(),
        }
    }
}

// ══════════════════════════════════════════════════════
// خطأ الأمان
// ══════════════════════════════════════════════════════

#[derive(Debug)]
pub enum SafetyError {
    PathTraversal(String),
    BlockedCommand(String),
    WorkspaceEscape(String),
}

impl std::fmt::Display for SafetyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::PathTraversal(s) => write!(f, "Path traversal: {}", s),
            Self::BlockedCommand(s) => write!(f, "Blocked command: {}", s),
            Self::WorkspaceEscape(s) => write!(f, "Workspace escape: {}", s),
        }
    }
}

// ══════════════════════════════════════════════════════
// Failure Classification
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum FailureKind {
    SyntaxError,
    ImportError,
    AssertionError,
    TypeError,
    CollectionError,
    BuildError,
    DatabaseError,
    NodeTestError,
    FlaskConcurrency,
    InfraError, // v6.4: connection error / rate limit / pip timeout
    PatchError, // v6.6: search block not found / validation failed
    Unknown,
}

impl FailureKind {
    pub fn classify(stderr: &str) -> Self {
        let s = stderr;
        // Patch errors — highest priority after Infra (context mismatch)
        // يجب أن يسبق كل تصنيف آخر لأن patch failure يختلط مع أخطاء أخرى
        if s.contains("search block not found")
            || s.contains("patch_file validation failed")
            || s.contains("Patch changed too many lines")
            || s.contains("search block is empty")
        {
            return Self::PatchError;
        }
        // Infra errors — highest priority (never send to LLM)
        if s.contains("Connection error")
            || s.contains("rate limit")
            || s.contains("Rate limit")
            || s.contains("429")
            || s.contains("503")
            || s.contains("502")
            || s.contains("Timeout after")
            || s.contains("error sending request")
        {
            return Self::InfraError;
        }
        // Go errors
        if s.contains("undefined:")
            || s.contains("cannot use")
            || s.contains("no required module")
            || s.contains("cannot find package")
        {
            return Self::TypeError;
        }
        if s.contains("syntax error:") && (s.contains(".go:") || s.contains("unexpected")) {
            return Self::SyntaxError;
        }
        if s.contains("FAIL	") || s.contains("--- FAIL") {
            return Self::AssertionError;
        }
        // Rust project structure errors
        if s.contains("could not find `Cargo.toml`") || s.contains("could not find Cargo.toml") {
            return Self::BuildError;
        }
        // Rust assertion failures
        if s.contains("left") && s.contains("right") && s.contains("panicked") {
            return Self::AssertionError;
        }
        // Rust errors
        if s.contains("error[E") || (s.contains("error:") && s.contains("-->")) {
            // Rust type/borrow errors
            if s.contains("E0308") || s.contains("mismatched types") || s.contains("E0507") {
                return Self::TypeError;
            }
            return Self::BuildError;
        }
        if s.contains("thread") && s.contains("panicked") {
            return Self::AssertionError;
        }
        if s.contains("FAILED") && s.contains("test result:") {
            return Self::AssertionError;
        }
        // Python errors
        if s.contains("ModuleNotFoundError")
            || s.contains("ImportError while importing")
            || s.contains("No module named")
        {
            return Self::ImportError;
        }
        if s.contains("SyntaxError") || s.contains("was never closed") {
            return Self::SyntaxError;
        }
        if s.contains("collected 0 items") {
            return Self::CollectionError;
        }
        if s.contains("AttributeError") {
            return Self::TypeError;
        }
        if s.contains("TypeError") {
            return Self::TypeError;
        }
        if s.contains("AssertionError") {
            return Self::AssertionError;
        }
        if s.contains("error[E") || s.contains("error: ") && s.contains("-->") {
            return Self::BuildError;
        }
        if s.contains("LookupError")
            && (s.contains("flask") || s.contains("app_ctx") || s.contains("application context"))
            || s.contains("RuntimeError") && s.contains("Working outside of application context")
            || s.contains("RuntimeError") && s.contains("Working outside of request context")
            || s.contains("Push an application context")
        {
            return Self::FlaskConcurrency;
        }
        if s.contains("ReferenceError: test is not defined")
            || s.contains("ReferenceError: describe is not defined")
            || s.contains("ReferenceError: expect is not defined")
        {
            return Self::NodeTestError;
        }
        if s.contains("OperationalError")
            || s.contains("no such table")
            || s.contains("readonly database")
            || s.contains("sqlite3")
            || s.contains("sqlalchemy")
        {
            return Self::DatabaseError;
        }
        Self::Unknown
    }

    pub fn repair_hint(&self) -> &str {
        match self {
            Self::SyntaxError =>
                "SYNTAX ERROR: Fix syntax only. Do NOT change logic or reinstall packages.",
            Self::ImportError =>
                "IMPORT ERROR: Module not found. Either install it with pip or use stdlib alternative.",
            Self::AssertionError =>
                "ASSERTION ERROR: Logic is wrong. Fix the implementation, not the test.",
            Self::TypeError =>
                "TYPE ERROR: Wrong types used. Check function signatures and return types.",
            Self::CollectionError =>
                "COLLECTION ERROR: pytest found 0 tests. Ensure test functions start with test_",
            Self::BuildError =>
                "BUILD ERROR: Compilation failed. Fix the compile errors shown.",
            Self::NodeTestError =>
                "NODE TEST ERROR: Do NOT use Jest/Mocha syntax (test/describe/expect).                  Use only Node.js built-in assert module.                  Example: const assert = require('assert'); assert.strictEqual(add(2,3), 5);",
            Self::DatabaseError =>
                "DATABASE ERROR: The test database is not set up correctly.                  You MUST use this exact pattern in test_main.py:
                 
                 from sqlalchemy import create_engine
                 from sqlalchemy.orm import sessionmaker
                 from main import app, Base, get_db
                 from fastapi.testclient import TestClient
                 
                 SQLALCHEMY_TEST_URL = 'sqlite:///:memory:'
                 engine = create_engine(SQLALCHEMY_TEST_URL, connect_args={'check_same_thread': False})
                 TestingSessionLocal = sessionmaker(bind=engine)
                 
                 def override_get_db():
                     db = TestingSessionLocal()
                     try: yield db
                     finally: db.close()
                 
                 app.dependency_overrides[get_db] = override_get_db
                 Base.metadata.create_all(bind=engine)
                 client = TestClient(app)
                 
                 IMPORTANT: main.py must have get_db() as a dependency injection function.",
            Self::FlaskConcurrency =>
                "FLASK CONTEXT ERROR: Code is running outside Flask application context.                 
YOU MUST fix the test file using one of these patterns:                 

PATTERN A — pytest fixture (recommended):                 
  import pytest                 
  from main import app                 
  @pytest.fixture                 
  def client():                 
      app.config['TESTING'] = True                 
      with app.test_client() as c:                 
          yield c                 
  def test_route(client):                 
      r = client.get('/')                 
      assert r.status_code == 200                 

PATTERN B — app_context manually:                 
  with app.app_context():                 
      # code that needs app context                 

NEVER call db or app internals outside app context.",
            Self::PatchError =>
                "PATCH ERROR: The search block was not found in the file.                 You MUST read the current file content first, then use the EXACT text as the search block.                 Do NOT approximate or paraphrase. Copy the exact lines from the file.",
            Self::InfraError =>
                "INFRA ERROR: Network/API issue. No code fix needed — retry automatically.",
            Self::Unknown =>
                "Fix the errors shown above.",
        }
    }
}

impl ExecutionContext {
    pub fn load_hashes(&mut self, workspace: &std::path::Path) {
        let path = workspace.join(".sel_hashes");
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let h = line.trim();
                if !h.is_empty() {
                    self.successful_hashes.insert(h.to_string());
                }
            }
        }
    }

    pub fn save_hashes(&self, workspace: &std::path::Path) {
        let path = workspace.join(".sel_hashes");
        let content = self
            .successful_hashes
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let _ = std::fs::write(path, content);
    }
}

// ══════════════════════════════════════════════════════
// v5.1: Context Configuration
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ContextConfig {
    pub ref_file: Option<PathBuf>, // ملف مرجعي للأنواع والتوقيعات
    pub focus_paths: Vec<String>,  // مسارات لإعطاء أولوية أعلى
    pub max_context_files: usize,  // الحد الأقصى للملفات (50 بدل 20)
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            ref_file: None,
            focus_paths: vec![],
            max_context_files: 50, // رفع من 20 إلى 50
        }
    }
}

impl FailureKind {
    pub fn max_attempts(&self) -> u8 {
        match self {
            Self::PatchError => 2, // context mismatch — أعطِ فرصتين مع hint
            Self::InfraError => 0, // لا LLM repair — retry فقط
            Self::ImportError => 1,
            Self::NodeTestError => 1,
            Self::DatabaseError => 2,
            Self::FlaskConcurrency => 2,
            Self::SyntaxError => 3,
            Self::TypeError => 3,
            Self::AssertionError => 3,
            Self::BuildError => 3,
            Self::CollectionError => 2,
            Self::Unknown => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_repair_budget() {
        assert_eq!(FailureKind::ImportError.max_attempts(), 1);
        assert_eq!(FailureKind::SyntaxError.max_attempts(), 3);
        assert_eq!(FailureKind::DatabaseError.max_attempts(), 2);
        assert_eq!(FailureKind::Unknown.max_attempts(), 3);
        assert_eq!(FailureKind::InfraError.max_attempts(), 0);
        assert_eq!(FailureKind::PatchError.max_attempts(), 2);
        assert_eq!(
            FailureKind::classify("search block not found in 'app.ts'"),
            FailureKind::PatchError
        );
        assert_eq!(
            FailureKind::classify("patch_file validation failed: too many lines"),
            FailureKind::PatchError
        );
        // InfraError يجب أن يُصنَّف صح
        assert_eq!(
            FailureKind::classify("Connection error: timeout"),
            FailureKind::InfraError
        );
        assert_eq!(
            FailureKind::classify("Timeout after 120s"),
            FailureKind::InfraError
        );
    }
}

```

