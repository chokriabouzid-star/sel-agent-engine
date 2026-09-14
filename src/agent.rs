// src/agent.rs  v1.3: State Machine

use crate::{
    executor::SafeExecutor,
    protocol::Cmd,
    report::{stable_goal_hash, ExecutionOutcome, ExecutionReport},
    report_writer::ReportWriter,
    types::{AgentState, ContextConfig, ExecutionContext},
};
use anyhow::Result;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

pub struct Agent {
    state: AgentState,
    pub ctx: ExecutionContext,
    executor: SafeExecutor,
    pub llm: Box<dyn crate::llm::LLMProvider>,
    goal: String,
    plan: Vec<Cmd>,
    error_history: Vec<String>,
    repair_fingerprints: Vec<u64>, // Repair History Guard v1.2
    context_config: ContextConfig,
    #[allow(dead_code)]
    // v5.8: FailureMemory instance — currently loaded on-demand in state_handlers; field reserved for caching
    failure_memory: crate::memory::FailureMemory,
    initial_snapshot: Option<crate::snapshot::Snapshot>, // v7.5.1
    pub bench_mode: bool, // v7.9.8: skip EXPLAIN MODE in all bench runs
    goal_authorized_test_files: Vec<PathBuf>, // explicit existing tests allowed by user goal
    initial_goal_test_write_window_open: bool, // only during first execution before repair
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
            llm: Box::new(crate::llm::live::LiveProvider::from_env()),
            goal,
            plan: Vec::new(),
            error_history: Vec::new(),
            repair_fingerprints: Vec::new(),
            context_config,
            failure_memory: crate::memory::FailureMemory::load(),
            initial_snapshot: None,
            bench_mode: false,
            goal_authorized_test_files: Vec::new(),
            initial_goal_test_write_window_open: false,
        }
    }
    pub fn new_with_model(
        _api_key: String,
        _model_alias: String,
        workspace: PathBuf,
        goal: String,
        max_repairs: u8,
        context_config: ContextConfig,
        llm: Box<dyn crate::llm::LLMProvider>,
    ) -> Self {
        Self {
            state: AgentState::Planning,
            ctx: ExecutionContext::new(max_repairs),
            executor: SafeExecutor::new(workspace, 120),
            llm,
            goal,
            plan: Vec::new(),
            error_history: Vec::new(),
            repair_fingerprints: Vec::new(),
            context_config,
            failure_memory: crate::memory::FailureMemory::load(),
            initial_snapshot: None,
            bench_mode: false,
            goal_authorized_test_files: Vec::new(),
            initial_goal_test_write_window_open: false,
        }
    }

    pub fn call_stats(&self) -> crate::llm::LlmCallStats {
        self.llm.get_stats()
    }

    pub fn repair_count(&self) -> usize {
        self.ctx.repair_attempts as usize
    }

    pub fn failed_reason(&self) -> Option<String> {
        if let crate::types::AgentState::Failed(ref reason) = self.state {
            Some(reason.clone())
        } else {
            None
        }
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
            match reqwest::Client::new()
                .post(&url)
                .json(&body)
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await
            {
                Ok(resp) if !resp.status().is_success() => {
                    tracing::debug!(
                        status = %resp.status(),
                        "observatory event: non-success response"
                    );
                }
                Err(e) => {
                    tracing::debug!(error = %e, "observatory event send failed");
                }
                _ => {}
            }
        });
    }

    pub fn mutation_score(&self) -> f64 {
        if self.ctx.mutations_total > 0 {
            self.ctx.mutations_killed as f64 / self.ctx.mutations_total as f64
        } else {
            -1.0
        }
    }

    //
    // All planning, execution, and repair logic delegated to
    // state_handlers.rs (v7.6 refactor)
    //

    //

    pub async fn run(&mut self) -> Result<()> {
        self.ctx.start_time = Some(std::time::Instant::now());
        self.ctx.bench_mode = self.bench_mode; // v8.1: Sync bench mode to context
        self.executor.bench_mode = self.bench_mode; // v8.5: Sync bench mode to executor for test protection

        // v7.6.1: Activate replay mode on executor to prevent internet access
        if self.llm.mode() == "replay" {
            self.executor.replay_mode = true;
            eprintln!("[TRACE] Replay mode ON  network operations disabled in executor");
        }

        self.send_event("start", None, None, None, None);
        // v5.8.1:   cache    run
        let ws = self.executor.workspace.clone();
        let cache_path = ws.join(".sel_hashes");
        if cache_path.exists() {
            let _ = std::fs::remove_file(&cache_path);
            println!("     Cache cleared  fresh start");
        }

        // v6.3: ScaffoldEngine     LLM
        let scaffold =
            crate::scaffold_engine::prepare(&ws, &self.goal, self.llm.mode() == "replay").await;

        if !scaffold.ready
            && matches!(
                scaffold.kind,
                crate::scaffold_engine::ProjectKind::TypeScript
                    | crate::scaffold_engine::ProjectKind::Python
            )
        {
            let msg = if scaffold.logic_hint.is_empty() {
                format!("scaffold failed for {:?}", scaffold.kind)
            } else {
                format!(
                    "scaffold failed for {:?}: {}",
                    scaffold.kind, scaffold.logic_hint
                )
            };
            eprintln!("   ❌ Scaffold failed: {}", msg);
            self.send_event("scaffold_failed", None, None, None, None);
            return Err(anyhow::anyhow!(msg));
        }

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

        // v7.9.10: Initial snapshot AFTER scaffold  so bugfix scaffold files
        // are committed to git baseline and survive the stash cycle.
        // Previously this was BEFORE scaffold_engine, which stashed away
        // the scaffold_files written by the bench runner.
        // v8.1: Preflight Workspace Scan before taking the initial snapshot
        let mut has_tests = false;
        self.executor.protected_test_files.clear();

        for entry in walkdir::WalkDir::new(&ws)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let name = entry.file_name().to_string_lossy();

                if self.executor.is_spec_file(&name) {
                    has_tests = true;
                    self.executor
                        .protected_test_files
                        .insert(entry.path().to_path_buf());
                    continue;
                }

                if name.ends_with(".rs") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if content.contains("#[test]") || content.contains("#[cfg(test)]") {
                            has_tests = true;
                        }
                    }
                }
            }
        }

        if !self.executor.protected_test_files.is_empty() {
            eprintln!(
                "[TRACE] protected test files snapshot: {}",
                self.executor.protected_test_files.len()
            );
        }

        self.goal_authorized_test_files =
            extract_goal_authorized_test_files(&self.goal, &self.executor.protected_test_files);
        self.initial_goal_test_write_window_open = !self.goal_authorized_test_files.is_empty();
        self.executor
            .set_goal_authorized_test_files(&self.goal_authorized_test_files);
        self.executor.set_allow_goal_test_writes(false);
        self.executor.set_broken_authorized_test_repair(false);

        if !self.goal_authorized_test_files.is_empty() {
            let names = self
                .goal_authorized_test_files
                .iter()
                .map(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| p.to_string_lossy().to_string())
                })
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "   🔓 Goal-authorized existing test edits (initial plan only): {}",
                names
            );
        }
        if !has_tests {
            // In bench_mode, benchmarks orchestrate tests on their own
            if self.bench_mode {
                // Do not enforce preflight checks
            } else {
                let is_creation_task = [
                    "create a",
                    "create the",
                    "implement a",
                    "implement the",
                    "write a",
                    "write the",
                    "build a",
                    "build the",
                ]
                .iter()
                .any(|s| self.goal.to_lowercase().contains(s));

                if !is_creation_task {
                    eprintln!("\u{26a0}\u{fe0f}  No test files found in workspace  agent cannot verify fixes");
                    // We removed the bench_mode Err return here since bench_mode skips this entirely
                } else {
                    println!("   \u{2139}\u{fe0f}  Creation task  no pre-existing tests required. Proceeding...");
                }
            }
        }

        // FIX H-05: only commit scaffold_baseline if workspace has no HEAD yet
        // Avoids polluting user's existing git history
        let has_head = std::process::Command::new("git")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(["rev-parse", "HEAD"])
            .current_dir(&ws)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !has_head {
            let _ = std::process::Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["init", "-q"])
                .current_dir(&ws)
                .output();
            let _ = std::process::Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["config", "user.name", "SEL Agent"])
                .current_dir(&ws)
                .output();
            let _ = std::process::Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["config", "user.email", "sel@local.test"])
                .current_dir(&ws)
                .output();
            let _ = std::process::Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .arg("add")
                .arg(".")
                .current_dir(&ws)
                .output();
            let _ = std::process::Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["commit", "-m", "scaffold_baseline", "--allow-empty"])
                .current_dir(&ws)
                .output();
            eprintln!("[TRACE] H-05: created initial git baseline (workspace had no HEAD)");
        } else {
            eprintln!("[TRACE] H-05: workspace already has git history — skipping scaffold_baseline commit");
        }
        self.initial_snapshot = Some(crate::snapshot::Snapshot::take(&ws));

        loop {
            match self.state.clone() {
                AgentState::Planning => {
                    match crate::state_handlers::do_planning_with_goal_authorized_test_edits(
                        &mut self.ctx,
                        self.llm.as_ref(),
                        &self.goal,
                        &self.executor.workspace,
                        &self.context_config,
                        !self.goal_authorized_test_files.is_empty(),
                    )
                    .await
                    {
                        Ok((commands, new_state)) => {
                            self.plan = commands;
                            self.state = new_state;
                        }
                        Err(e) => {
                            self.state = AgentState::Failed(e.to_string());
                        }
                    }
                }

                AgentState::Executing => {
                    let allow_goal_test_writes = self.initial_goal_test_write_window_open;
                    self.executor
                        .set_allow_goal_test_writes(allow_goal_test_writes);

                    let mut snapshot = crate::snapshot::Snapshot::take(&self.executor.workspace);
                    let execute_result = crate::state_handlers::do_executing(
                        &mut self.ctx,
                        &self.executor,
                        &self.plan,
                    )
                    .await;

                    // Protect any newly created test files after execution so repair cannot spawn duplicate tests
                    for entry in walkdir::WalkDir::new(&self.executor.workspace)
                        .into_iter()
                        .filter_map(|e| e.ok())
                    {
                        if entry.file_type().is_file() {
                            let name = entry.file_name().to_string_lossy();
                            if self.executor.is_spec_file(&name) {
                                self.executor
                                    .protected_test_files
                                    .insert(entry.path().to_path_buf());
                            }
                        }
                    }

                    if allow_goal_test_writes {
                        // أبقِ الـ window مفتوحًا إذا كان الفشل بسبب الـ test file المُصرَّح به
                        let authorized_file_still_broken = self.ctx.failed_steps.iter().any(|f| {
                            self.goal_authorized_test_files.iter().any(|auth| {
                                f.stderr.contains(
                                    auth.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                                )
                            })
                        });

                        if !authorized_file_still_broken {
                            self.executor.set_allow_goal_test_writes(false);
                            self.initial_goal_test_write_window_open = false;
                            eprintln!("[TRACE] goal-authorized test write window closed");
                        } else {
                            eprintln!(
                                "[TRACE] goal-authorized test write window kept open (authorized file still broken)"
                            );
                        }
                    }

                    match execute_result {
                        Ok(new_state) => {
                            snapshot.commit();
                            self.state = new_state;
                        }
                        Err(e) => {
                            snapshot.rollback();
                            self.state = AgentState::Failed(e.to_string());
                        }
                    }
                }

                AgentState::Repairing => {
                    // إذا كان الـ test file المُصرَّح به هو مصدر الفشل، افتح repair window
                    let broken_authorized = !self.goal_authorized_test_files.is_empty()
                        && self.ctx.failed_steps.iter().any(|f| {
                            self.goal_authorized_test_files.iter().any(|auth| {
                                let name = auth.file_name().and_then(|n| n.to_str()).unwrap_or("");
                                !name.is_empty() && f.stderr.contains(name)
                            })
                        });

                    if broken_authorized {
                        self.executor.set_broken_authorized_test_repair(true);
                        eprintln!("[TRACE] goal-authorized broken test repair window opened");
                    } else {
                        self.executor.set_broken_authorized_test_repair(false);
                    }

                    // FIX C-02: sync protected_test_files into ctx so checklist can guard autofix
                    self.ctx.protected_test_files = self.executor.protected_test_files.clone();

                    match crate::state_handlers::do_repairing(
                        &mut self.ctx,
                        self.llm.as_ref(),
                        &self.goal,
                        &self.executor.workspace,
                        &self.context_config,
                        &mut self.repair_fingerprints,
                        &mut self.error_history,
                    )
                    .await
                    {
                        Ok((commands, new_state)) => {
                            self.plan = commands;
                            self.state = new_state;
                        }
                        Err(e) => {
                            self.state = AgentState::Failed(e.to_string());
                        }
                    }
                    self.executor.set_broken_authorized_test_repair(false);
                }
                AgentState::WaitingForUserInput(msg) => {
                    // v8.0: In bench mode, skip EXPLAIN MODE immediately using env var or struct field
                    if self.bench_mode
                        || std::env::var("SEL_BENCH_MODE").is_ok()
                        || !std::io::stdin().is_terminal()
                        || !std::io::stdout().is_terminal()
                    {
                        println!(
                            "     [Bench] Repairs exhausted  marking failed (skip EXPLAIN MODE)"
                        );
                        self.state = AgentState::Failed("max_repairs_bench".into());
                        continue;
                    }

                    println!("\n  [EXPLAIN MODE] Agent is stuck and needs help!");
                    println!("{}", msg);

                    println!("\n Type a hint to guide the agent, or type 'abort' to fail:");
                    use std::io::Write;
                    print!("> ");
                    let _ = std::io::stdout().flush();

                    let mut input = String::new();
                    if let Err(e) = std::io::stdin().read_line(&mut input) {
                        println!("    Input read error: {}", e);
                        self.state = AgentState::Failed("Aborted due to input error".to_string());
                        continue;
                    }
                    let input = input.trim();

                    if input.eq_ignore_ascii_case("abort") {
                        self.state = AgentState::Failed("Aborted by user".to_string());
                    } else {
                        // Append user hint to error_history so the LLM sees it as feedback
                        let hint = format!("\nUSER HINT: {}\n", input);
                        {
                            self.error_history.push(hint);
                        }

                        // Give the agent one more repair attempt
                        self.ctx.repair_attempts = self.ctx.max_repairs;
                        self.ctx.max_repairs += 1;
                        self.state = AgentState::Repairing;
                        println!("    Resuming repair with your hint...");
                    }
                }

                AgentState::Done => {
                    if let Some(mut snap) = self.initial_snapshot.take() {
                        snap.commit();
                    }

                    let repairs = self.ctx.repair_attempts.saturating_sub(1);

                    let elapsed = self
                        .ctx
                        .start_time
                        .map(|s| s.elapsed().as_secs())
                        .unwrap_or(0);
                    let ms = self.mutation_score();
                    let stats = self.call_stats();
                    let telemetry = compute_report_telemetry(
                        &self.ctx,
                        stats.tokens_in as u64,
                        stats.tokens_out as u64,
                        stats.successful_calls as u64,
                    );
                    let cost = crate::cost::CostTracker::new();
                    cost.add_usage(stats.tokens_in, stats.tokens_out, stats.successful_calls);
                    let model = if stats.last_model.is_empty() {
                        std::env::var("SEL_MODEL")
                            .unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string())
                    } else {
                        stats.last_model.clone()
                    };

                    self.ctx.tokens_used = telemetry.total_tokens;

                    record_pattern_outcome(
                        &self.executor.workspace,
                        &self.ctx,
                        &self.error_history,
                        &self.plan,
                        true,
                        None,
                    );

                    let _ = report_run(ReportRunInput {
                        workspace: &self.executor.workspace,
                        goal: &self.goal,
                        success: true,
                        repairs: repairs as i64,
                        duration_secs: elapsed,
                        mutation_score: ms,
                        mutations_equivalent: self.ctx.mutations_equivalent as u64,
                        mode: self.llm.mode(),
                        autofix_count: self.ctx.autofix_count as u64,
                        tests_passed: self.ctx.tests_passed,
                        provider_model: &model,
                        llm_calls: stats.successful_calls as u64,
                        tokens_in: stats.tokens_in as u64,
                        tokens_out: stats.tokens_out as u64,
                        total_tokens: telemetry.total_tokens,
                        avg_tokens_per_task: telemetry.avg_tokens_per_task,
                        avg_context_tokens: telemetry.avg_context_tokens,
                        avg_selected_files: telemetry.avg_selected_files,
                        context_reduction_pct: telemetry.context_reduction_pct,
                        force_include_dropped_count: telemetry.force_include_dropped_count,
                        failure_reason: None,
                        plan_risk_triggered: self.ctx.plan_risk_triggered,
                        replan_count: self.ctx.replan_count as u64,
                        plan_risk_reasons: self.ctx.plan_risk_reasons.clone(),
                    })
                    .await;

                    if let Some(mut snap) = self.initial_snapshot.take() {
                        snap.commit();
                    }

                    self.executor.set_allow_goal_test_writes(false);
                    self.executor.set_broken_authorized_test_repair(false);
                    self.executor.clear_goal_authorized_test_files();

                    cost.print_summary(&model);

                    return Ok(());
                }
                AgentState::Failed(reason) => {
                    println!("\n Agent failed: {}", reason);
                    println!("SEL_FAILED: {}", reason.lines().next().unwrap_or("unknown"));
                    // FIX C-01-B: commit (not rollback) on failure to preserve agent-created files.
                    // Per-attempt snapshots inside Executing already handle rolling back failed attempts.
                    // Rolling back initial_snapshot would destroy files the agent created during the session.
                    if let Some(mut snap) = self.initial_snapshot.take() {
                        snap.commit();
                    }
                    let repairs = self.ctx.repair_attempts as i64;
                    let elapsed = self
                        .ctx
                        .start_time
                        .map(|s| s.elapsed().as_secs())
                        .unwrap_or(0);
                    let ms = self.mutation_score();
                    let stats = self.call_stats();
                    let telemetry = compute_report_telemetry(
                        &self.ctx,
                        stats.tokens_in as u64,
                        stats.tokens_out as u64,
                        stats.successful_calls as u64,
                    );
                    let cost = crate::cost::CostTracker::new();
                    cost.add_usage(stats.tokens_in, stats.tokens_out, stats.successful_calls);
                    let model = if stats.last_model.is_empty() {
                        std::env::var("SEL_MODEL")
                            .unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string())
                    } else {
                        stats.last_model.clone()
                    };

                    self.ctx.tokens_used = telemetry.total_tokens;

                    record_pattern_outcome(
                        &self.executor.workspace,
                        &self.ctx,
                        &self.error_history,
                        &self.plan,
                        false,
                        Some(&reason),
                    );

                    let _ = report_run(ReportRunInput {
                        workspace: &self.executor.workspace,
                        goal: &self.goal,
                        success: false,
                        repairs,
                        duration_secs: elapsed,
                        mutation_score: ms,
                        mutations_equivalent: self.ctx.mutations_equivalent as u64,
                        mode: self.llm.mode(),
                        autofix_count: self.ctx.autofix_count as u64,
                        tests_passed: self.ctx.tests_passed,
                        provider_model: &model,
                        llm_calls: stats.successful_calls as u64,
                        tokens_in: stats.tokens_in as u64,
                        tokens_out: stats.tokens_out as u64,
                        total_tokens: telemetry.total_tokens,
                        avg_tokens_per_task: telemetry.avg_tokens_per_task,
                        avg_context_tokens: telemetry.avg_context_tokens,
                        avg_selected_files: telemetry.avg_selected_files,
                        context_reduction_pct: telemetry.context_reduction_pct,
                        force_include_dropped_count: telemetry.force_include_dropped_count,
                        failure_reason: Some(reason.clone()),
                        plan_risk_triggered: self.ctx.plan_risk_triggered,
                        replan_count: self.ctx.replan_count as u64,
                        plan_risk_reasons: self.ctx.plan_risk_reasons.clone(),
                    })
                    .await;

                    self.executor.set_allow_goal_test_writes(false);
                    self.executor.set_broken_authorized_test_repair(false);
                    self.executor.clear_goal_authorized_test_files();

                    cost.print_summary(&model);

                    return Err(anyhow::anyhow!("SEL_FAILED"));
                }
            }
        }
    }
}

fn latest_pattern_error(
    ctx: &crate::types::ExecutionContext,
    error_history: &[String],
    failure_reason: Option<&str>,
) -> String {
    // Priority 1: failed_steps الحالية
    if let Some(err) = ctx.failed_steps.iter().rev().find_map(|f| {
        let s = f.stderr.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    }) {
        return err;
    }

    // Priority 2: last_failed_steps من الجولة السابقة
    if let Some(err) = ctx.last_failed_steps.iter().rev().find_map(|f| {
        let s = f.stderr.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    }) {
        return err;
    }

    // Priority 3: error_history
    if let Some(err) = error_history.iter().rev().find_map(|e| {
        let s = e.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    }) {
        return err;
    }

    if let Some(reason) = failure_reason {
        let reason = reason.trim();
        if !reason.is_empty() {
            return reason.to_string();
        }
    }

    String::new()
}

fn infer_pattern_route(stderr: &str) -> crate::pattern_library::RepairRoute {
    crate::pattern_library::infer_route_from_stderr(stderr)
}

fn summarize_fix_plan(plan: &[crate::protocol::Cmd]) -> Option<String> {
    let mut parts = Vec::new();

    for cmd in plan.iter().take(6) {
        match cmd {
            crate::protocol::Cmd::PatchFile { path, .. } => {
                parts.push(format!("patch_file:{}", path));
            }
            crate::protocol::Cmd::WriteFile { path, .. } => {
                parts.push(format!("write_file:{}", path));
            }
            crate::protocol::Cmd::AppendFile { path, .. } => {
                parts.push(format!("append_file:{}", path));
            }
            crate::protocol::Cmd::Run { command } => {
                let short: String = command.chars().take(40).collect();
                parts.push(format!("run:{}", short));
            }
            crate::protocol::Cmd::RunTests { target } => {
                parts.push(format!("run_tests:{}", target));
            }
            crate::protocol::Cmd::DeleteFile { path } => {
                parts.push(format!("delete_file:{}", path));
            }
            crate::protocol::Cmd::ReadFile { .. }
            | crate::protocol::Cmd::Mkdir { .. }
            | crate::protocol::Cmd::Done { .. } => {}
        }
    }

    if parts.is_empty() {
        None
    } else {
        let joined = parts.join(" | ");
        Some(joined.chars().take(240).collect())
    }
}

fn record_pattern_outcome(
    workspace: &Path,
    ctx: &crate::types::ExecutionContext,
    error_history: &[String],
    plan: &[crate::protocol::Cmd],
    success: bool,
    failure_reason: Option<&str>,
) {
    let stderr = latest_pattern_error(ctx, error_history, failure_reason);
    if stderr.trim().is_empty() {
        return;
    }

    let language = crate::pattern_library::infer_language_from_workspace(workspace);
    let route = infer_pattern_route(&stderr);
    let example_fix = if success {
        summarize_fix_plan(plan)
    } else {
        None
    };

    let mut lib = crate::pattern_library::PatternLibrary::load();
    lib.record_outcome(language, &stderr, route, success, example_fix);
    lib.save();
}

fn extract_goal_authorized_test_files(
    goal: &str,
    protected: &std::collections::HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let goal_lower = goal.to_lowercase();

    // FIX H-11: check negation BEFORE edit intent
    // "Do not modify test_x.py" must NOT open authorization window
    let negation_phrases = [
        "do not modify",
        "do not edit",
        "do not change",
        "do not touch",
        "do not update",
        "do not write",
        "do not alter",
        "don't modify",
        "don't edit",
        "don't change",
        "don't touch",
        "don't update",
        "don't write",
        "don't alter",
        "never modify",
        "never edit",
        "never change",
        "never touch",
        "must not modify",
        "must not edit",
        "must not change",
        "without modifying",
        "without editing",
        "without changing",
        "leave intact",
        "leave unchanged",
        "keep unchanged",
    ];

    // FIX H-11: check per-file negation — "only implement X, do not modify test_Y"
    let mentions_edit_intent = ["add", "update", "modify", "edit", "write"]
        .iter()
        .any(|verb| goal_lower.contains(verb));
    let mentions_tests = goal_lower.contains("test");

    if !mentions_edit_intent || !mentions_tests {
        return Vec::new();
    }

    // If any global negation phrase present → no authorization at all
    if negation_phrases.iter().any(|neg| goal_lower.contains(neg)) {
        eprintln!("[TRACE] H-11: negation detected in goal — no test files authorized for editing");
        return Vec::new();
    }

    let mut matches = protected
        .iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_lowercase();
            if !goal_lower.contains(&name) {
                return None;
            }

            // FIX H-11: check per-file negation — "do not modify test_foo.py"
            let file_negated = negation_phrases.iter().any(|neg| {
                // look for negation near the filename in the goal
                goal_lower
                    .find(&name)
                    .map(|pos| {
                        let window_start = pos.saturating_sub(60);
                        let window = &goal_lower[window_start..pos + name.len()];
                        window.contains(neg.split_whitespace().next().unwrap_or(""))
                    })
                    .unwrap_or(false)
            });

            if file_negated {
                eprintln!(
                    "[TRACE] H-11: file {:?} negated in goal — not authorized",
                    name
                );
                None
            } else {
                Some(path.clone())
            }
        })
        .collect::<Vec<_>>();

    matches.sort();
    matches.dedup();
    matches
}

struct ReportRunInput<'a> {
    workspace: &'a Path,
    goal: &'a str,
    success: bool,
    repairs: i64,
    duration_secs: u64,
    mutation_score: f64,
    mutations_equivalent: u64,
    mode: &'a str,
    autofix_count: u64,
    tests_passed: bool,
    provider_model: &'a str,
    llm_calls: u64,
    tokens_in: u64,
    tokens_out: u64,
    total_tokens: u64,
    avg_tokens_per_task: u64,
    avg_context_tokens: u64,
    avg_selected_files: u64,
    context_reduction_pct: u8,
    force_include_dropped_count: u64,
    failure_reason: Option<String>,

    // v9.2.1: Plan Risk Telemetry
    plan_risk_triggered: bool,
    replan_count: u64,
    plan_risk_reasons: Vec<String>,
}

async fn report_run(input: ReportRunInput<'_>) -> Result<()> {
    let timestamp_utc = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

    let report = ExecutionReport {
        version: env!("CARGO_PKG_VERSION").to_string(),
        goal: input.goal.chars().take(200).collect::<String>(),
        goal_hash: stable_goal_hash(input.goal),
        workspace: input.workspace.display().to_string(),
        timestamp_utc: timestamp_utc.clone(),
        duration_secs: input.duration_secs,
        mode: input.mode.to_string(),
        outcome: if input.success {
            ExecutionOutcome::Pass
        } else {
            ExecutionOutcome::Fail
        },
        repair_attempts: input.repairs,
        autofix_count: input.autofix_count,
        tests_passed: input.tests_passed,
        mutation_score: input.mutation_score,
        mutations_equivalent: input.mutations_equivalent as u32,
        provider_model: input.provider_model.to_string(),
        llm_calls: input.llm_calls,
        tokens_in: input.tokens_in,
        tokens_out: input.tokens_out,
        total_tokens: input.total_tokens,
        avg_tokens_per_task: input.avg_tokens_per_task,
        avg_context_tokens: input.avg_context_tokens,
        avg_selected_files: input.avg_selected_files,
        context_reduction_pct: input.context_reduction_pct,
        force_include_dropped_count: input.force_include_dropped_count,
        failure_reason: input.failure_reason.clone(),
        plan_risk_triggered: input.plan_risk_triggered,
        replan_count: input.replan_count,
        plan_risk_reasons: input.plan_risk_reasons.clone(),
    };

    if let Err(e) = ReportWriter::default().write(&report) {
        tracing::debug!(error = %e, "failed to write execution report");
    }

    let body = serde_json::json!({
        "goal": report.goal,
        "success": input.success,
        "repairs": input.repairs,
        "duration_secs": input.duration_secs,
        "mutation_score": input.mutation_score,
        "model": input.provider_model,
        "mode": input.mode,
        "failure_reason": input.failure_reason,
        "goal_hash": report.goal_hash,
        "timestamp_utc": timestamp_utc
    });

    let base_url =
        std::env::var("SEL_OBSERVATORY").unwrap_or_else(|_| "http://localhost:8777".to_string());
    let url = format!("{}/api/runs", base_url.trim_end_matches('/'));

    let client = reqwest::Client::new();
    let _ = client
        .post(url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;

    Ok(())
}

// v9.3.0 Wave 1.5: Extracted telemetry computation for testability
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedTelemetry {
    pub total_tokens: u64,
    pub avg_tokens_per_task: u64,
    pub avg_context_tokens: u64,
    pub avg_selected_files: u64,
    pub context_reduction_pct: u8,
    pub force_include_dropped_count: u64,
}

pub(crate) fn compute_report_telemetry(
    ctx: &crate::types::ExecutionContext,
    tokens_in: u64,
    tokens_out: u64,
    successful_calls: u64,
) -> ComputedTelemetry {
    let total_tokens = tokens_in + tokens_out;
    let avg_tokens_per_task = total_tokens.checked_div(successful_calls).unwrap_or(0);
    let avg_context_tokens = ctx
        .context_tokens_total
        .checked_div(ctx.context_budget_samples as u64)
        .unwrap_or(0);
    let avg_selected_files = ctx
        .context_files_total
        .checked_div(ctx.context_budget_samples as u64)
        .unwrap_or(0);
    let context_reduction_pct = if ctx.context_tokens_before_total > 0 {
        let saved = ctx
            .context_tokens_before_total
            .saturating_sub(ctx.context_tokens_total);
        ((saved * 100) / ctx.context_tokens_before_total) as u8
    } else {
        0
    };
    ComputedTelemetry {
        total_tokens,
        avg_tokens_per_task,
        avg_context_tokens,
        avg_selected_files,
        context_reduction_pct,
        force_include_dropped_count: ctx.force_include_dropped_count,
    }
}

#[cfg(test)]
mod evidence_telemetry {
    use super::*;
    use crate::types::ExecutionContext;

    #[test]
    fn evidence_total_tokens_equals_in_plus_out() {
        let ctx = ExecutionContext::new(5);
        let t = compute_report_telemetry(&ctx, 200, 100, 3);
        assert_eq!(t.total_tokens, 300);
    }

    #[test]
    fn evidence_avg_tokens_per_task_zero_when_no_calls() {
        let ctx = ExecutionContext::new(5);
        let t = compute_report_telemetry(&ctx, 0, 0, 0);
        assert_eq!(t.avg_tokens_per_task, 0);
    }

    #[test]
    fn evidence_avg_tokens_per_task_computed_correctly() {
        let ctx = ExecutionContext::new(5);
        let t = compute_report_telemetry(&ctx, 600, 400, 5);
        assert_eq!(t.total_tokens, 1000);
        assert_eq!(t.avg_tokens_per_task, 200);
    }

    #[test]
    fn evidence_context_averages_computed_from_samples() {
        let mut ctx = ExecutionContext::new(5);
        ctx.context_tokens_total = 3000;
        ctx.context_files_total = 12;
        ctx.context_budget_samples = 3;
        ctx.context_tokens_before_total = 5000;

        let t = compute_report_telemetry(&ctx, 100, 50, 2);
        assert_eq!(t.avg_context_tokens, 1000);
        assert_eq!(t.avg_selected_files, 4);
        assert_eq!(t.context_reduction_pct, 40);
    }

    #[test]
    fn evidence_context_averages_zero_when_no_samples() {
        let ctx = ExecutionContext::new(5);
        let t = compute_report_telemetry(&ctx, 100, 50, 1);
        assert_eq!(t.avg_context_tokens, 0);
        assert_eq!(t.avg_selected_files, 0);
        assert_eq!(t.context_reduction_pct, 0);
    }

    #[test]
    fn evidence_force_include_dropped_count_propagates() {
        let mut ctx = ExecutionContext::new(5);
        ctx.force_include_dropped_count = 3;
        let t = compute_report_telemetry(&ctx, 0, 0, 0);
        assert_eq!(t.force_include_dropped_count, 3);
    }

    #[test]
    fn evidence_context_reduction_pct_zero_when_no_before() {
        let mut ctx = ExecutionContext::new(5);
        ctx.context_tokens_before_total = 0;
        ctx.context_tokens_total = 100;
        ctx.context_budget_samples = 1;
        let t = compute_report_telemetry(&ctx, 0, 0, 0);
        assert_eq!(t.context_reduction_pct, 0);
    }
}

#[cfg(test)]
mod extract_goal_authorized_tests {
    use super::*;
    use std::collections::HashSet;

    fn make_protected(names: &[&str]) -> HashSet<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    /// H-11: "do not modify test_x.py" must return empty
    #[test]
    fn h11_do_not_modify_returns_empty() {
        let protected = make_protected(&["test_main.py"]);
        let result = extract_goal_authorized_test_files(
            "Implement solution.py only. Do not modify test_main.py.",
            &protected,
        );
        assert!(
            result.is_empty(),
            "H-11 FAIL: 'do not modify' should prevent authorization, got: {:?}",
            result
        );
    }

    /// H-11: "never modify test_x.py" must return empty
    #[test]
    fn h11_never_modify_returns_empty() {
        let protected = make_protected(&["test_main.py"]);
        let result = extract_goal_authorized_test_files(
            "Write solution.py. Never modify test_main.py.",
            &protected,
        );
        assert!(
            result.is_empty(),
            "H-11 FAIL: 'never modify' should prevent authorization, got: {:?}",
            result
        );
    }

    /// H-11: "don't edit test_x.py" must return empty
    #[test]
    fn h11_dont_edit_returns_empty() {
        let protected = make_protected(&["test_foo.py"]);
        let result = extract_goal_authorized_test_files(
            "Add feature to main.py. Don't edit test_foo.py.",
            &protected,
        );
        assert!(
            result.is_empty(),
            "H-11 FAIL: don't edit should prevent authorization, got: {:?}",
            result
        );
    }

    /// H-11: "only implement X" with no test mention returns empty
    #[test]
    fn h11_only_implement_no_test_mention_returns_empty() {
        let protected = make_protected(&["test_main.py"]);
        let result = extract_goal_authorized_test_files(
            "Only implement the slugify function in solution.py.",
            &protected,
        );
        assert!(
            result.is_empty(),
            "H-11 FAIL: no test mention should return empty, got: {:?}",
            result
        );
    }

    /// H-11: legitimate authorization without negation works
    #[test]
    fn h11_legitimate_authorization_works() {
        let protected = make_protected(&["test_main.py"]);
        let result = extract_goal_authorized_test_files(
            "Update test_main.py to add test cases for the new feature.",
            &protected,
        );
        assert!(
            !result.is_empty(),
            "H-11 FAIL: legitimate authorization should work"
        );
    }

    /// H-11: "without modifying tests" returns empty
    #[test]
    fn h11_without_modifying_returns_empty() {
        let protected = make_protected(&["test_main.py"]);
        let result = extract_goal_authorized_test_files(
            "Fix the bug in main.py without modifying test_main.py.",
            &protected,
        );
        assert!(
            result.is_empty(),
            "H-11 FAIL: 'without modifying' should prevent authorization, got: {:?}",
            result
        );
    }
}

#[cfg(test)]
mod c01b_h11_regression_tests {
    use std::path::PathBuf;

    fn contains_path(got: &[PathBuf], expected: &PathBuf) -> bool {
        got.iter().any(|p| p == expected || p.ends_with(expected))
    }

    #[test]
    fn c01b_failed_branch_commits_initial_snapshot_not_rolls_back() {
        let src = include_str!("agent.rs");

        let start = src
            .find("AgentState::Failed(reason) =>")
            .expect("AgentState::Failed(reason) branch not found");

        let end = std::cmp::min(src.len(), start + 2200);
        let window = &src[start..end];

        assert!(
            window.contains("initial_snapshot.take()"),
            "C-01-B FAIL: Failed branch does not take initial_snapshot"
        );

        assert!(
            window.contains("snap.commit()"),
            "C-01-B FAIL: Failed branch must commit initial_snapshot to preserve agent-created files"
        );

        assert!(
            !window.contains("snap.rollback()"),
            "C-01-B FAIL: Failed branch must not rollback initial_snapshot"
        );
    }

    #[test]
    fn h11_do_not_modify_named_test_file_is_not_authorized() {
        let protected = [PathBuf::from("tests/test_public_api.py")];

        let goal = "Implement the feature in src/lib.rs. Do not modify tests/test_public_api.py.";

        let got = super::extract_goal_authorized_test_files(
            goal,
            &protected
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
        );

        assert!(
            got.is_empty(),
            "H-11 FAIL: negated instruction authorized a protected test file: {:?}",
            got
        );
    }

    #[test]
    fn h11_dont_edit_named_test_file_is_not_authorized() {
        let protected = [PathBuf::from("tests/test_public_api.py")];

        let goal = "Fix the implementation only; don't edit tests/test_public_api.py.";

        let got = super::extract_goal_authorized_test_files(
            goal,
            &protected
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
        );

        assert!(
            got.is_empty(),
            "H-11 FAIL: \"don't edit\" authorized a protected test file: {:?}",
            got
        );
    }

    #[test]
    fn h11_passive_must_not_be_modified_is_not_authorized() {
        let protected = [PathBuf::from("tests/test_public_api.py")];

        let goal =
            "tests/test_public_api.py must not be modified. Change only the production code.";

        let got = super::extract_goal_authorized_test_files(
            goal,
            &protected
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
        );

        assert!(
            got.is_empty(),
            "H-11 FAIL: passive negation authorized a protected test file: {:?}",
            got
        );
    }

    #[test]
    fn h11_positive_explicit_test_edit_still_authorizes() {
        let protected = [PathBuf::from("tests/test_public_api.py")];

        let goal = "Modify tests/test_public_api.py to cover the new expected behavior.";

        let got = super::extract_goal_authorized_test_files(
            goal,
            &protected
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
        );

        assert!(
            contains_path(&got, &protected[0]),
            "H-11 FAIL: positive explicit authorization did not authorize the test file. got={:?}",
            got
        );
    }
}
