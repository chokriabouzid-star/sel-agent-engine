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
    failure_memory: crate::memory::FailureMemory, // v5.8
    initial_snapshot: Option<crate::snapshot::Snapshot>, // v7.5.1
    pub bench_mode: bool,                         // v7.9.8: skip EXPLAIN MODE in all bench runs
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

        {
            let _ = std::process::Command::new("git")
                .arg("add")
                .arg(".")
                .current_dir(&ws)
                .output();
            let _ = std::process::Command::new("git")
                .args(["commit", "-m", "scaffold_baseline", "--allow-empty"])
                .current_dir(&ws)
                .output();
        }
        self.initial_snapshot = Some(crate::snapshot::Snapshot::take(&ws));

        loop {
            match self.state.clone() {
                AgentState::Planning => {
                    match crate::state_handlers::do_planning(
                        &mut self.ctx,
                        self.llm.as_ref(),
                        &self.goal,
                        &self.executor.workspace,
                        &self.context_config,
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
                    let mut snapshot = crate::snapshot::Snapshot::take(&self.executor.workspace);
                    match crate::state_handlers::do_executing(
                        &mut self.ctx,
                        &self.executor,
                        &self.plan,
                    )
                    .await
                    {
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
                    let repairs = self.ctx.repair_attempts.saturating_sub(1);

                    let elapsed = self
                        .ctx
                        .start_time
                        .map(|s| s.elapsed().as_secs())
                        .unwrap_or(0);
                    let ms = self.mutation_score();
                    let stats = self.call_stats();
                    let total_tokens = stats.tokens_in as u64 + stats.tokens_out as u64;
                    let avg_tokens_per_task = if stats.successful_calls > 0 {
                        total_tokens / stats.successful_calls as u64
                    } else {
                        0
                    };
                    let cost = crate::cost::CostTracker::new();
                    cost.add_usage(stats.tokens_in, stats.tokens_out, stats.successful_calls);
                    let model = if stats.last_model.is_empty() {
                        std::env::var("SEL_MODEL")
                            .unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string())
                    } else {
                        stats.last_model.clone()
                    };

                    self.ctx.tokens_used = total_tokens;

                    let avg_context_tokens = if self.ctx.context_budget_samples > 0 {
                        {
                            self.ctx.context_tokens_total / self.ctx.context_budget_samples as u64
                        }
                    } else {
                        {
                            0
                        }
                    };
                    let avg_selected_files = if self.ctx.context_budget_samples > 0 {
                        {
                            self.ctx.context_files_total / self.ctx.context_budget_samples as u64
                        }
                    } else {
                        {
                            0
                        }
                    };
                    let context_reduction_pct = if self.ctx.context_tokens_before_total > 0 {
                        {
                            let saved = self
                                .ctx
                                .context_tokens_before_total
                                .saturating_sub(self.ctx.context_tokens_total);
                            ((saved * 100) / self.ctx.context_tokens_before_total) as u8
                        }
                    } else {
                        {
                            0
                        }
                    };

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
                        mode: self.llm.mode(),
                        autofix_count: self.ctx.autofix_count as u64,
                        tests_passed: self.ctx.tests_passed,
                        provider_model: &model,
                        llm_calls: stats.successful_calls as u64,
                        tokens_in: stats.tokens_in as u64,
                        tokens_out: stats.tokens_out as u64,
                        total_tokens,
                        avg_tokens_per_task,
                        avg_context_tokens,
                        avg_selected_files,
                        context_reduction_pct,
                        force_include_dropped_count: self.ctx.force_include_dropped_count,
                        failure_reason: None,
                        plan_risk_triggered: self.ctx.plan_risk_triggered,
                        replan_count: self.ctx.replan_count as u64,
                        plan_risk_reasons: self.ctx.plan_risk_reasons.clone(),
                    })
                    .await;

                    cost.print_summary(&model);

                    return Ok(());
                }
                AgentState::Failed(reason) => {
                    println!("\n Agent failed: {}", reason);
                    println!("SEL_FAILED: {}", reason.lines().next().unwrap_or("unknown"));
                    if let Some(mut snap) = self.initial_snapshot.take() {
                        snap.rollback();
                    }
                    let repairs = self.ctx.repair_attempts as i64;
                    let elapsed = self
                        .ctx
                        .start_time
                        .map(|s| s.elapsed().as_secs())
                        .unwrap_or(0);
                    let ms = self.mutation_score();
                    let stats = self.call_stats();
                    let total_tokens = stats.tokens_in as u64 + stats.tokens_out as u64;
                    let avg_tokens_per_task = if stats.successful_calls > 0 {
                        total_tokens / stats.successful_calls as u64
                    } else {
                        0
                    };
                    let cost = crate::cost::CostTracker::new();
                    cost.add_usage(stats.tokens_in, stats.tokens_out, stats.successful_calls);
                    let model = if stats.last_model.is_empty() {
                        std::env::var("SEL_MODEL")
                            .unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string())
                    } else {
                        stats.last_model.clone()
                    };

                    self.ctx.tokens_used = total_tokens;

                    let avg_context_tokens = if self.ctx.context_budget_samples > 0 {
                        {
                            self.ctx.context_tokens_total / self.ctx.context_budget_samples as u64
                        }
                    } else {
                        {
                            0
                        }
                    };
                    let avg_selected_files = if self.ctx.context_budget_samples > 0 {
                        {
                            self.ctx.context_files_total / self.ctx.context_budget_samples as u64
                        }
                    } else {
                        {
                            0
                        }
                    };
                    let context_reduction_pct = if self.ctx.context_tokens_before_total > 0 {
                        {
                            let saved = self
                                .ctx
                                .context_tokens_before_total
                                .saturating_sub(self.ctx.context_tokens_total);
                            ((saved * 100) / self.ctx.context_tokens_before_total) as u8
                        }
                    } else {
                        {
                            0
                        }
                    };

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
                        mode: self.llm.mode(),
                        autofix_count: self.ctx.autofix_count as u64,
                        tests_passed: self.ctx.tests_passed,
                        provider_model: &model,
                        llm_calls: stats.successful_calls as u64,
                        tokens_in: stats.tokens_in as u64,
                        tokens_out: stats.tokens_out as u64,
                        total_tokens,
                        avg_tokens_per_task,
                        avg_context_tokens,
                        avg_selected_files,
                        context_reduction_pct,
                        force_include_dropped_count: self.ctx.force_include_dropped_count,
                        failure_reason: Some(reason.clone()),
                        plan_risk_triggered: self.ctx.plan_risk_triggered,
                        replan_count: self.ctx.replan_count as u64,
                        plan_risk_reasons: self.ctx.plan_risk_reasons.clone(),
                    })
                    .await;

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

struct ReportRunInput<'a> {
    workspace: &'a Path,
    goal: &'a str,
    success: bool,
    repairs: i64,
    duration_secs: u64,
    mutation_score: f64,
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
