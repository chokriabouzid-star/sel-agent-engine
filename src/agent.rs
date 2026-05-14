// src/agent.rs — v1.3: State Machine

use crate::{
    executor::SafeExecutor,
    protocol::Cmd,
    types::{AgentState, ContextConfig, ExecutionContext},
};
use anyhow::Result;
use std::path::PathBuf;
use std::io::IsTerminal;

pub struct Agent {
    state: AgentState,
    pub ctx: ExecutionContext,
    executor: SafeExecutor,
    pub llm: Box<dyn crate::llm::LLMProvider>,
    goal: String,
    plan: Vec<Cmd>,
    previous_error: Option<String>,
    repair_fingerprints: Vec<u64>, // Repair History Guard v1.2
    context_config: ContextConfig,
    failure_memory: crate::memory::FailureMemory, // v5.8
    initial_snapshot: Option<crate::snapshot::Snapshot>, // v7.5.1
    pub bench_mode: bool, // v7.9.8: skip EXPLAIN MODE in all bench runs
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
            previous_error: None,
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
            previous_error: None,
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
    // All planning, execution, and repair logic delegated to
    // state_handlers.rs (v7.6 refactor)
    // ══════════════════════════════════════════════════════════

    // ══════════════════════════════════════════════════════════

    pub async fn run(&mut self) -> Result<()> {
        self.ctx.start_time = Some(std::time::Instant::now());

        // v7.6.1: Activate replay mode on executor to prevent internet access
        if self.llm.mode() == "replay" {
            self.executor.replay_mode = true;
            eprintln!("[TRACE] Replay mode ON — network operations disabled in executor");
        }

        self.send_event("start", None, None, None, None);
        // v5.8.1: امسح الـ cache في بداية كل run — كل جلسة تبدأ نظيفة
        let ws = self.executor.workspace.clone();
        let cache_path = ws.join(".sel_hashes");
        if cache_path.exists() {
            let _ = std::fs::remove_file(&cache_path);
            println!("   🗑  Cache cleared — fresh start");
        }

        // v6.3: ScaffoldEngine — يُجهّز البيئة قبل LLM
        let scaffold =
            crate::scaffold_engine::prepare(&ws, &self.goal, self.llm.mode() == "replay").await;
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

        // v7.9.10: Initial snapshot AFTER scaffold — so bugfix scaffold files
        // are committed to git baseline and survive the stash cycle.
        // Previously this was BEFORE scaffold_engine, which stashed away
        // the scaffold_files written by the bench runner.
        {
            let _ = std::process::Command::new("git").arg("add").arg(".")
                .current_dir(&ws).output();
            let _ = std::process::Command::new("git")
                .args(&["commit", "-m", "scaffold_baseline", "--allow-empty"])
                .current_dir(&ws).output();
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
                        &mut self.previous_error,
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
                    if self.bench_mode || std::env::var("SEL_BENCH_MODE").is_ok() || !std::io::stdin().is_terminal() {
                        println!("   ⏭  [Bench] Repairs exhausted — marking failed (skip EXPLAIN MODE)");
                        self.state = AgentState::Failed("max_repairs_bench".into());
                        continue;
                    }

                    println!("\n⏸  [EXPLAIN MODE] Agent is stuck and needs help!");
                    println!("{}", msg);

                    println!("\n💡 Type a hint to guide the agent, or type 'abort' to fail:");
                    use std::io::Write;
                    print!("> ");
                    std::io::stdout().flush().unwrap();

                    let mut input = String::new();
                    if let Err(e) = std::io::stdin().read_line(&mut input) {
                        println!("   ❌ Input read error: {}", e);
                        self.state = AgentState::Failed("Aborted due to input error".to_string());
                        continue;
                    }
                    let input = input.trim();

                    if input.eq_ignore_ascii_case("abort") {
                        self.state = AgentState::Failed("Aborted by user".to_string());
                    } else {
                        // Append user hint to previous_error so the LLM sees it as feedback
                        let hint = format!("\nUSER HINT: {}\n", input);
                        if let Some(ref mut prev) = self.previous_error {
                            prev.push_str(&hint);
                        } else {
                            self.previous_error = Some(hint);
                        }
                        
                        // Give the agent one more repair attempt
                        self.ctx.repair_attempts = self.ctx.max_repairs;
                        self.ctx.max_repairs += 1;
                        self.state = AgentState::Repairing;
                        println!("   🔄 Resuming repair with your hint...");
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
                    let _ = report_run(&self.goal, true, repairs as i64, elapsed, ms).await;
                    
                    let stats = self.call_stats();
                    let cost = crate::cost::CostTracker::new();
                    cost.add_usage(stats.tokens_in, stats.tokens_out, stats.successful_calls);
                    let model = if stats.last_model.is_empty() {
                        std::env::var("SEL_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string())
                    } else {
                        stats.last_model.clone()
                    };
                    cost.print_summary(&model);
                    
                    return Ok(());
                }
                AgentState::Failed(reason) => {
                    println!("\n❌ Agent failed: {}", reason);
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
                    let _ = report_run(&self.goal, false, repairs, elapsed, ms).await;
                    
                    let stats = self.call_stats();
                    let cost = crate::cost::CostTracker::new();
                    cost.add_usage(stats.tokens_in, stats.tokens_out, stats.successful_calls);
                    let model = if stats.last_model.is_empty() {
                        std::env::var("SEL_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string())
                    } else {
                        stats.last_model.clone()
                    };
                    cost.print_summary(&model);
                    
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
