// src/state_handlers.rs  v7.6: State Handlers
use crate::executor::SafeExecutor;
use crate::llm::{LLMProvider, LLMRequest};
use crate::protocol::Cmd;
use crate::types::{AgentState, ContextConfig, ExecutionContext, FailedStep, FailureKind, Message};
use anyhow::Result;
use std::path::Path;

fn plan_risk_feedback(workspace: &Path, commands: &[Cmd]) -> Vec<String> {
    let enabled = std::env::var_os("SEL_DISABLE_PLAN_RISK").is_none();
    plan_risk_feedback_with_flag(workspace, commands, enabled)
}

fn plan_risk_feedback_with_flag(workspace: &Path, commands: &[Cmd], enabled: bool) -> Vec<String> {
    if !enabled {
        return Vec::new();
    }

    if std::env::var_os("SEL_GOAL_AUTHORIZED_TESTS").is_some() {
        eprintln!("[TRACE] plan_risk: skipped for explicit goal-authorized existing test edits");
        // Still check plan size even when skipping full plan_risk
        return crate::decision::check_plan_size(commands);
    }

    let plan_risk = crate::decision::evaluate_plan_risk(workspace, commands);
    if plan_risk.should_replan() {
        plan_risk.feedback_lines()
    } else {
        Vec::new()
    }
}

fn record_plan_risk_telemetry(
    ctx: &mut ExecutionContext,
    commands: &[Cmd],
    plan_risk_issues: &[String],
) {
    if plan_risk_issues.is_empty() {
        return;
    }

    ctx.plan_risk_triggered = true;
    ctx.plan_risk_reasons = plan_risk_issues.to_vec();

    if ctx.commands_before_replan == 0 {
        ctx.commands_before_replan = commands.len();
    }
}

//
// PLANNING
//

pub async fn do_planning(
    ctx: &mut ExecutionContext,
    llm: &dyn LLMProvider,
    goal: &str,
    workspace: &Path,
    config: &ContextConfig,
) -> Result<(Vec<Cmd>, AgentState)> {
    if let Some(reason) = crate::decision::validate_goal(goal) {
        return Ok((Vec::new(), AgentState::Failed(reason.to_string())));
    }

    // v9.2.1: reset plan risk telemetry for this planning session
    ctx.plan_risk_triggered = false;
    ctx.plan_risk_reasons.clear();
    ctx.replan_count = 0;
    ctx.commands_before_replan = 0;

    let elapsed = ctx
        .start_time
        .map(|s| s.elapsed().as_secs_f32())
        .unwrap_or(0.0);
    eprintln!("\n[{:.1}s] 🧠 Planning...", elapsed);
    let ecm = crate::environment::EnvironmentCapabilities::probe();
    let prompt = build_planning_prompt(goal, workspace, config, &ecm);

    match plan_with_resilience(llm, prompt, ctx.bench_mode).await {
        Ok(commands) => {
            let commands_count = commands.len();
            let env = crate::constraint_engine::ProjectEnv::detect(workspace);
            let state = crate::constraint_engine::ProjectState::scan(workspace);
            let mut commands = match crate::constraint_engine::apply(commands, &env, &state) {
                crate::constraint_engine::ConstraintResult::Ok(filtered) => {
                    if filtered.len() < commands_count {
                        println!(
                            "   ⚙️  Constraint Engine: filtered {} commands",
                            commands_count - filtered.len()
                        );
                    }
                    filtered
                }
                crate::constraint_engine::ConstraintResult::Fatal(reason) => {
                    println!("   ❌ Constraint Engine: {}", reason);
                    return Ok((Vec::new(), AgentState::Failed(reason)));
                }
            };
            println!("   ✓ {} commands", commands.len());

            let mut issues = crate::decision::validate_plan_integrity(&commands);
            issues.extend(crate::decision::validate_rust_bootstrap_plan(
                workspace, &commands,
            ));
            let patch_issues = crate::decision::validate_patch_uniqueness(workspace, &commands);
            issues.extend(patch_issues);
            issues.extend(crate::decision::validate_protected_writes(&commands));

            let plan_risk_issues = plan_risk_feedback(workspace, &commands);
            record_plan_risk_telemetry(ctx, &commands, &plan_risk_issues);
            issues.extend(plan_risk_issues);

            if !issues.is_empty() {
                match replan_with_feedback(ctx, llm, goal, workspace, config, commands, issues)
                    .await
                {
                    Ok(valid_commands) => {
                        println!("   ✅ Final plan: {} commands\n", valid_commands.len());
                        Ok((valid_commands, AgentState::Executing))
                    }
                    Err(e) => {
                        println!("   ❌ Replan failed: {}", e);
                        Ok((Vec::new(), AgentState::Failed(e)))
                    }
                }
            } else {
                // v8.4: Dedup write_file — keep only LAST write per path
                {
                    let mut last_write: std::collections::HashMap<String, usize> =
                        std::collections::HashMap::new();
                    for (i, cmd) in commands.iter().enumerate() {
                        if let crate::protocol::Cmd::WriteFile { path, .. } = cmd {
                            last_write.insert(path.clone(), i);
                        }
                    }
                    let mut keep = vec![true; commands.len()];
                    for (i, cmd) in commands.iter().enumerate() {
                        if let crate::protocol::Cmd::WriteFile { path, .. } = cmd {
                            if last_write.get(path) != Some(&i) {
                                keep[i] = false;
                                println!(
                                    "   ⚠️  Dedup: skipping earlier write_file for '{}'",
                                    path
                                );
                            }
                        }
                    }
                    let had_dups = keep.iter().any(|k| !k);
                    if had_dups {
                        commands = commands
                            .into_iter()
                            .zip(keep)
                            .filter(|(_, k)| *k)
                            .map(|(c, _)| c)
                            .collect();
                    }
                }
                println!("   ✓ All patches unique\n");
                Ok((commands, AgentState::Executing))
            }
        }
        Err(e) => {
            println!("   ❌ Plan parse failed: {}", e);
            Ok((Vec::new(), AgentState::Failed(e)))
        }
    }
}

fn build_planning_prompt(
    goal: &str,
    workspace: &Path,
    config: &ContextConfig,
    ecm: &crate::environment::EnvironmentCapabilities,
) -> String {
    let env_context = ecm.to_planning_context();
    let constraints = ecm.derive_constraints();
    let goal_clarity = crate::decision::GoalClarity::analyze(goal);
    let clarity_instruction = goal_clarity.planning_hint();
    let advisory_hints = crate::decision::goal_advisory_hints(goal);
    let lang_hint = crate::decision::build_lang_hint(workspace);
    let ws_ctx = crate::decision::build_workspace_context(workspace);
    let existing_files = if !ws_ctx.is_empty() {
        ws_ctx
    } else if config.ref_file.is_some() {
        crate::decision::build_skeleton_context(workspace)
    } else {
        String::new()
    };

    let ref_context = crate::decision::build_ref_context(config);

    // v7.9.9 P1: Explicit repair instruction if files exist
    let repair_instruction = if !existing_files.is_empty() {
        "\n  EXISTING FILES ARE PROVIDED. Your job is to FIX the bugs in them.\n\
         - DO NOT delete or overwrite these files from scratch.\n\
         - Use patch_file to apply precise fixes.\n\
         - Keep existing correct logic and only change what is broken.\n\
         - EXCEPTION: For Cargo.toml dependency changes, use write_file with the COMPLETE file content.\n\
           Example: write_file Cargo.toml with [package] + [dependencies] sections complete.\n"
    } else {
        ""
    };

    let clarity_hint_block = if clarity_instruction.is_empty() {
        String::new()
    } else {
        format!("\nGOAL CLARITY HINT:\n{}\n", clarity_instruction)
    };

    let advisory_hint_block = if advisory_hints.is_empty() {
        String::new()
    } else {
        format!(
            "\nGOAL ADVISORY HINTS:\n- {}\n",
            advisory_hints.join("\n- ")
        )
    };

    let goal_lower = goal.to_lowercase();

    let go_http_planning_hint = if goal_lower.contains("httptest")
        || (goal_lower.contains("http") && goal_lower.contains("go"))
    {
        "\nGO HTTP TESTING HINT:\n- In Go, do NOT register routes only inside main().\n- Extract route registration into: func setupRouter() http.Handler\n- Use http.NewServeMux() inside setupRouter()\n- In tests, call httptest.NewServer(setupRouter())\n- NEVER redefine setupRouter() in main_test.go if it already exists in main.go\n"
            .to_string()
    } else {
        String::new()
    };

    let ts_plan_compaction_hint = if goal_lower.contains("typescript")
        || goal_lower.contains("node.js")
        || goal_lower.contains("jest")
        || goal_lower.contains(".ts")
    {
        "\nTYPESCRIPT PLAN COMPACTION HINT:\n- Prefer ONE write_file per file instead of many patch_file operations.\n- For small TypeScript tasks, keep the whole plan under 6 commands when possible.\n- Avoid multiple patches to the same .ts or .test.ts file in one plan.\n"
            .to_string()
    } else {
        String::new()
    };

    let thinking_prompt = format!(
        "\n\n## Required Analysis\n\
Before writing the JSON plan, think step-by-step inside <think>...</think> tags:\n\
<think>\n\
- Is this a bugfix task (files already exist) or a new feature task?\n\
- What exactly is broken in the existing files?\n\
- How can I fix it using patch_file without rewriting everything?\n\
- What imports are MANDATORY for this language?\n\
- What edge cases must the tests cover?\n\
</think>\n\
After </think>, output ONLY the ```json plan. Nothing else outside the JSON block.\n\
CRITICAL PROTOCOL REMINDER:\n\
- Every plan MUST contain run_tests BEFORE done (non-negotiable)\n\
- pip install: use venv/bin/pip install <pkg>\n\
- Cargo.toml: use write_file with complete content when adding dependencies\n\
{}{}{}{}{}",
        repair_instruction,
        clarity_hint_block,
        advisory_hint_block,
        go_http_planning_hint,
        ts_plan_compaction_hint
    );

    crate::constitution::CONSTITUTION.to_string()
        + &format!(
            "{}{}{}\n{}\n{}{}\nGoal: {}\nProvide the complete execution plan.",
            existing_files, ref_context, lang_hint, env_context, constraints, thinking_prompt, goal
        )
}

async fn plan_with_resilience(
    llm: &dyn LLMProvider,
    base_prompt: String,
    bench_mode: bool,
) -> Result<Vec<Cmd>, String> {
    const MAX_RETRIES: u8 = 3;
    let mut last_error = String::new();
    for attempt in 0..=MAX_RETRIES {
        let prompt = if attempt == 0 {
            base_prompt.clone()
        } else {
            let error_msg = if last_error.is_empty() {
                "previous response could not be parsed.".to_string()
            } else {
                last_error.clone()
            };

            let specific_guidance = if error_msg.contains("PLAN ERROR") {
                "\n\n CRITICAL ERROR  PLAN REJECTED \nYour plan is MISSING the required run_tests command.\nYou MUST add this command BEFORE done:\n{\"type\": \"run_tests\", \"target\": \"auto\"}\nDo NOT omit it. Do NOT change anything else."
            } else {
                ""
            };

            format!(
                "{}\n\n                     WARNING  PROTOCOL RETRY {}/{}: {}\n                     STRICT RULES:{} \n                     1. Output ONLY a ```json block  zero text outside it.\n                     2. Keep all content strings SHORT (< 40 chars per line).\n                     3. Use ONLY single quotes inside Python/shell code.\n                     4. No raw newlines inside JSON strings  use \\n instead.\n                     5. No special characters that break JSON strings.",
                base_prompt, attempt, MAX_RETRIES, error_msg, specific_guidance
            )
        };

        let req = LLMRequest {
            system: crate::llm::get_system_prompt(bench_mode),
            messages: vec![Message::user(prompt)],
            temperature: 0.1,
            seed: Some(42),
            model: "default".into(),
        };

        match llm.complete(req).await {
            Ok(response) => match crate::protocol::parse(&response.content) {
                Ok(mut plan) => {
                    if let Err(e) = crate::protocol::validate_test_order(&mut plan) {
                        if attempt < MAX_RETRIES {
                            last_error = format!("PLAN ERROR: {}", e);
                            println!("    {}", last_error);
                        } else {
                            return Err(format!("Plan validation failed: {}", e));
                        }
                    } else {
                        if attempt > 0 {
                            println!("   ✅ Protocol retry {} succeeded.", attempt);
                        }
                        return Ok(plan.commands);
                    }
                }
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        let err_msg = e.to_string();
                        last_error = err_msg.clone();
                        println!(
                            "   {} (attempt {}/{})",
                            crate::llm::classify_json_error(&err_msg),
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
            },
            Err(e) => return Err(e.to_string()),
        }
    }
    unreachable!()
}

fn replan_with_feedback<'a>(
    ctx: &'a mut ExecutionContext,
    llm: &'a dyn LLMProvider,
    goal: &'a str,
    workspace: &'a Path,
    config: &'a ContextConfig,
    original_plan: Vec<Cmd>,
    issues: Vec<String>,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = std::result::Result<Vec<Cmd>, String>> + Send + 'a>,
> {
    Box::pin(async move {
        ctx.replan_attempts += 1;
        ctx.replan_count += 1;
        if ctx.commands_before_replan == 0 {
            ctx.commands_before_replan = original_plan.len();
        }

        if ctx.replan_attempts > 2 {
            println!("   ⚠️  Max replan attempts (2) reached - proceeding with original plan");
            return Ok(original_plan);
        }

        println!("\n   🔧 Replan {}/2 - plan issues:", ctx.replan_attempts);
        for issue in &issues {
            println!("       {}", issue);
        }

        let plan_risk_issues: Vec<&str> = issues
            .iter()
            .filter(|i| i.starts_with("PLAN RISK:"))
            .map(|i| i.as_str())
            .collect();

        let other_issues: Vec<&str> = issues
            .iter()
            .filter(|i| !i.starts_with("PLAN RISK:"))
            .map(|i| i.as_str())
            .collect();

        let plan_risk_block = if plan_risk_issues.is_empty() {
            String::new()
        } else {
            format!(
                "PLAN RISK VIOLATIONS (fix these first):\n{}\n\
                 - Do NOT use write_file on existing source files; prefer patch_file for surgical fixes.\n\
                 - Do NOT modify existing test files; fix implementation files instead.\n\
                 - Do NOT use delete_file unless it is strictly unavoidable and justified.\n\n",
                plan_risk_issues.join("\n")
            )
        };

        let other_issues_block = if other_issues.is_empty() {
            String::new()
        } else {
            format!(
                "PLAN VALIDATION ISSUES:\n{}\n\
                 - Each patch_file search block must appear EXACTLY ONCE in the target file.\n\
                 - If search block is not found and the file does not exist yet, use write_file instead.\n\
                 - If search block appears multiple times, add more surrounding context lines.\n\
                 - Copy patch_file search text VERBATIM from the file (case-sensitive, exact whitespace).\n\n",
                other_issues.join("\n")
            )
        };

        let feedback = format!(
            "PLAN REJECTED - fix the following issues before execution.\n\n{}{}Provide corrected execution plan.",
            plan_risk_block,
            other_issues_block,
        );

        let existing_files = {
            let ws_ctx = crate::decision::build_workspace_context(workspace);
            if !ws_ctx.is_empty() {
                ws_ctx
            } else if config.ref_file.is_some() {
                crate::decision::build_skeleton_context(workspace)
            } else {
                String::new()
            }
        };

        let ref_context = crate::decision::build_ref_context(config);
        let lang_hint = crate::decision::build_lang_hint(workspace);
        let prompt = crate::constitution::CONSTITUTION.to_string()
            + &format!(
                "{}{}{}\nGoal: {}\n\nFEEDBACK:\n{}",
                existing_files, ref_context, lang_hint, goal, feedback
            );

        match plan_with_resilience(llm, prompt, ctx.bench_mode).await {
            Ok(candidate_plan) => {
                let commands_count = candidate_plan.len();
                let env = crate::constraint_engine::ProjectEnv::detect(workspace);
                let project_state = crate::constraint_engine::ProjectState::scan(workspace);

                let candidate_plan =
                    match crate::constraint_engine::apply(candidate_plan, &env, &project_state) {
                        crate::constraint_engine::ConstraintResult::Ok(filtered) => {
                            if filtered.len() < commands_count {
                                println!(
                                    "   ⚙️  Constraint Engine: filtered {} commands",
                                    commands_count - filtered.len()
                                );
                            }
                            filtered
                        }
                        crate::constraint_engine::ConstraintResult::Fatal(reason) => {
                            return Err(reason);
                        }
                    };

                let mut new_issues = crate::decision::validate_plan_integrity(&candidate_plan);
                new_issues.extend(crate::decision::validate_rust_bootstrap_plan(
                    workspace,
                    &candidate_plan,
                ));
                new_issues.extend(crate::decision::validate_patch_uniqueness(
                    workspace,
                    &candidate_plan,
                ));
                new_issues.extend(crate::decision::validate_protected_writes(&candidate_plan));
                new_issues.extend(plan_risk_feedback(workspace, &candidate_plan));
                let new_plan_risk_issues: Vec<String> = new_issues
                    .iter()
                    .filter(|i| i.starts_with("PLAN RISK:"))
                    .cloned()
                    .collect();
                record_plan_risk_telemetry(ctx, &candidate_plan, &new_plan_risk_issues);

                if new_issues.is_empty() {
                    println!("   ✅ Replan successful - plan issues resolved");
                    Ok(candidate_plan)
                } else {
                    replan_with_feedback(
                        ctx,
                        llm,
                        goal,
                        workspace,
                        config,
                        candidate_plan,
                        new_issues,
                    )
                    .await
                }
            }
            Err(e) => Err(e),
        }
    })
}

fn edited_path_from_cmd(cmd: &Cmd) -> Option<&str> {
    match cmd {
        Cmd::WriteFile { path, .. }
        | Cmd::AppendFile { path, .. }
        | Cmd::PatchFile { path, .. }
        | Cmd::DeleteFile { path } => Some(path.as_str()),
        _ => None,
    }
}

//
// EXECUTING
//

pub async fn do_executing(
    ctx: &mut ExecutionContext,
    executor: &SafeExecutor,
    plan: &[Cmd],
) -> Result<AgentState> {
    ctx.reset_for_repair();
    let total = plan.len();
    let mut i = 0;

    while i < total {
        let cmd = &plan[i];
        let elapsed = ctx
            .start_time
            .map(|s| s.elapsed().as_secs_f32())
            .unwrap_or(0.0);
        eprintln!(
            "[{:.1}s] ⚡ Executing  step {}/{} ({})",
            elapsed,
            i + 1,
            total,
            cmd.label()
        );

        let cmd_hash = cmd.hash();
        let is_pip = cmd.label().contains("pip");
        let venv_ok = executor.workspace.join("venv/bin/pip3").exists()
            || executor.workspace.join("venv/bin/pip").exists();
        let is_cargo_test =
            cmd.label().contains("cargo test") || cmd.label().contains("cargo check");

        let skip_allowed = !(cmd.is_run_tests()
            || cmd.is_write_file()
            || cmd.is_patch_file()
            || is_cargo_test
            || (is_pip && !venv_ok));

        let side_effect_still_exists = match cmd {
            Cmd::Run { command } => {
                let lc = command.to_lowercase();
                if lc.contains("go mod init") {
                    executor.workspace.join("go.mod").exists()
                } else if lc.contains("python3 -m venv") || lc.contains("python -m venv") {
                    executor.workspace.join("venv").exists()
                } else if lc.contains("npm install") {
                    executor.workspace.join("node_modules").exists()
                } else {
                    true
                }
            }
            _ => true,
        };

        if ctx.successful_hashes.contains(&cmd_hash.to_string())
            && skip_allowed
            && side_effect_still_exists
        {
            println!("   ⏭  Skipping: {} (already passed)", cmd.label());
            i += 1;
            continue;
        } else if ctx.successful_hashes.contains(&cmd_hash.to_string())
            && skip_allowed
            && !side_effect_still_exists
        {
            println!(
                "   ↩️  Re-running: {} (artifact missing after rollback)",
                cmd.label()
            );
        }

        if cmd.is_done() {
            if ctx.tests_passed {
                // Mutation Check v1.3
                if let Some(fail) = run_mutation_check(ctx, executor).await {
                    ctx.failed_steps.push(fail);
                    return Ok(AgentState::Repairing);
                } else {
                    ctx.save_hashes(&executor.workspace);
                    let msg = if let Cmd::Done { message } = cmd {
                        message
                    } else {
                        "Goal complete"
                    };
                    println!(
                        "\n {}",
                        if msg.is_empty() {
                            "Goal complete!"
                        } else {
                            msg
                        }
                    );
                    println!("SEL_SUCCESS");
                    return Ok(AgentState::Done);
                }
            } else {
                println!("   ❌ done rejected  tests must pass first");
                ctx.failed_steps.push(FailedStep {
                    step_index: i,
                    label: cmd.label(),
                    stderr: "done blocked: tests_passed = false".into(),
                    exit_code: 1,
                    culprit_file: None,
                });
                return Ok(AgentState::Repairing);
            }
        }

        match executor.run(cmd).await {
            Ok(r) if r.success => {
                let preview: String = r.stdout.chars().take(80).collect();
                if preview.is_empty() {
                    println!("   ✓ ({} ms)", r.duration_ms);
                } else {
                    println!("   ✓ ({} ms) → {}", r.duration_ms, preview);
                }

                if cmd.is_run_tests() {
                    ctx.tests_passed = true;
                }
                if let Cmd::Run { command } = cmd {
                    let lc = command.to_lowercase();
                    // v7.9.6: Trust exit code 0 from test runners  stdout may be empty
                    // (Jest outputs to stderr, npm wraps stdout, etc.)
                    if lc.contains("cargo test")
                        || lc.contains("go test")
                        || lc.contains("pytest")
                        || lc.contains("npm test")
                        || lc.contains("npx jest")
                    {
                        ctx.tests_passed = true;
                    }
                }
                if r.autofix_triggered {
                    ctx.autofix_count += 1;
                }
                if let Some(path) = edited_path_from_cmd(cmd) {
                    ctx.record_recent_edit(&executor.workspace, path);
                    ctx.invalidate_dependency_graph_cache();
                }
                ctx.successful_hashes.insert(cmd_hash.to_string());
            }
            Ok(r) => {
                let err: String = r.stderr.chars().take(3000).collect();
                println!("    {}", err);
                if cmd.is_run_tests() {
                    ctx.tests_passed = false;
                }
                ctx.failed_steps.push(FailedStep {
                    step_index: i,
                    label: cmd.label(),
                    stderr: err.clone(),
                    exit_code: r.exit_code,
                    culprit_file: FailedStep::extract_culprit(&err),
                });
                return Ok(AgentState::Repairing);
            }
            Err(e) => return Err(e),
        }
        i += 1;
    }

    if ctx.tests_passed {
        if let Some(fail) = run_mutation_check(ctx, executor).await {
            ctx.failed_steps.push(fail);
            Ok(AgentState::Repairing)
        } else {
            ctx.save_hashes(&executor.workspace);
            println!("\n✅ Goal complete! Tests passed.");
            println!("SEL_SUCCESS");
            Ok(AgentState::Done)
        }
    } else {
        Ok(AgentState::Repairing)
    }
}

async fn run_mutation_check(
    ctx: &mut ExecutionContext,
    executor: &SafeExecutor,
) -> Option<FailedStep> {
    if ctx.skip_mutation {
        return None;
    }
    let ws = &executor.workspace;

    let files: Vec<String> = walkdir::WalkDir::new(ws)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            e.path()
                .strip_prefix(ws)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })
        .filter(|f: &String| {
            !f.contains("test")
                && !f.contains("venv")
                && !f.contains("node_modules")
                && (f.ends_with(".py")
                    || f.ends_with(".go")
                    || f.ends_with(".rs")
                    || f.ends_with(".js")
                    || f.ends_with(".ts"))
        })
        .collect();

    if files.is_empty() {
        return None;
    }

    println!("   🧬 Mutation Check...");
    for src in &files {
        match executor.mutation_check(src).await {
            crate::executor::MutationResult::Weak(orig_line, mutd_line) => {
                let mutation_key = format!("{}|{}|{}", src, orig_line, mutd_line);
                let count = ctx
                    .mutation_survival_counts
                    .entry(mutation_key)
                    .or_insert(0);
                *count += 1;

                if *count >= 3 {
                    println!(
                        "     🧬 Equivalent Mutant detected (survived {} times)  skipping",
                        *count
                    );
                    ctx.mutations_total += 1;
                    ctx.mutations_equivalent += 1; // Counted separately, no longer inflates killed count!
                    continue;
                }

                ctx.mutations_total += 1;
                println!(
                    "   🧬 Survived mutation (Attempt {}): [{}]  [{}]",
                    count, orig_line, mutd_line
                );
                ctx.last_mutation_context = Some(crate::types::MutationContext {
                    surviving: format!(
                        "File: {}\nOriginal: {}\nMutation: {}",
                        src, orig_line, mutd_line
                    ),
                });
                return Some(FailedStep {
                    step_index: 0,
                    label: "mutation_check".into(),
                    stderr: format!(
                        "Mutation survived in {}: tests didn't catch change from '{}' to '{}'",
                        src, orig_line, mutd_line
                    ),
                    exit_code: 1,
                    culprit_file: Some(src.to_string()),
                });
            }
            crate::executor::MutationResult::Strong => {
                ctx.mutations_total += 1;
                ctx.mutations_killed += 1;
            }
            crate::executor::MutationResult::Uncompilable(orig, mutd) => {
                println!(
                    "     🧬 Mutation produced a compile error: [{}] -> [{}] — skipping",
                    orig, mutd
                );
                // Count toward total attempted mutations but NOT killed, as the tests didn't run.
                ctx.mutations_total += 1;
            }
            crate::executor::MutationResult::Skipped(reason) => {
                println!("     🧬 Mutation skipped for {}: {}", src, reason);
            }
        }
    }
    None
}

fn error_fingerprint(stderr: &str) -> u64 {
    stderr
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64))
}

fn trailing_same_fingerprint_count(history: &[u64], current: u64) -> usize {
    history
        .iter()
        .rev()
        .take_while(|&&fp| fp == current)
        .count()
}

fn recent_constitution_violation_count(error_history: &[String], current_error: &str) -> usize {
    let current = usize::from(current_error.contains("CONSTITUTION_VIOLATION:no-modify-tests"));

    current
        + error_history
            .iter()
            .rev()
            .take(4)
            .filter(|e| e.contains("CONSTITUTION_VIOLATION:no-modify-tests"))
            .count()
}

const MAX_REPAIR_PROMPT_CHARS: usize = 24_000;
const MAX_ATTEMPT_INFO_CHARS: usize = 3_000;
const MAX_HINTS_CHARS: usize = 1_600;
const MAX_MUTATION_NOTE_CHARS: usize = 1_200;
const MAX_FAILED_STEPS_CHARS: usize = 4_000;
const MAX_FILES_CONTEXT_CHARS: usize = 14_000;
const MAX_REF_CONTEXT_CHARS: usize = 2_000;
const MAX_CULPRIT_CONTENT_CHARS: usize = 4_000;

fn truncate_for_prompt(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    let take_n = max_chars.saturating_sub(1);
    let mut out: String = s.chars().take(take_n).collect();
    out.push('…');
    out
}

fn shrink_section_to_fit(section: &mut String, overflow: &mut usize, min_keep: usize) {
    if *overflow == 0 {
        return;
    }

    let current = section.chars().count();
    if current <= min_keep {
        return;
    }

    let reducible = current - min_keep;
    let reduce = reducible.min(*overflow);
    let new_len = current.saturating_sub(reduce);
    *section = truncate_for_prompt(section, new_len);
    *overflow = overflow.saturating_sub(reduce);
}

struct RepairPromptSections<'a> {
    goal: &'a str,
    attempt_bundle: &'a str,
    combined_hints: &'a str,
    mutation_note: &'a str,
    failed_steps: &'a str,
    files_context: &'a str,
    ref_context: &'a str,
    culprit_contents: &'a str,
}

fn assemble_repair_prompt(sections: &RepairPromptSections<'_>) -> String {
    crate::constitution::CONSTITUTION.to_string()
        + &format!(
            "Goal: {}\n\nATTEMPT INFO: {}\n\nHINTS: {}{}\n\nFAILED STEPS:\n{}\n\nCURRENT FILES:\n{}{}{}\nFix ALL issues.",
            sections.goal,
            sections.attempt_bundle,
            sections.combined_hints,
            sections.mutation_note,
            sections.failed_steps,
            sections.files_context,
            sections.ref_context,
            sections.culprit_contents
        )
}

fn build_budgeted_repair_prompt(sections: &RepairPromptSections<'_>) -> String {
    let mut attempt_bundle = truncate_for_prompt(sections.attempt_bundle, MAX_ATTEMPT_INFO_CHARS);
    let mut combined_hints = truncate_for_prompt(sections.combined_hints, MAX_HINTS_CHARS);
    let mut mutation_note = truncate_for_prompt(sections.mutation_note, MAX_MUTATION_NOTE_CHARS);
    let mut failed_steps = truncate_for_prompt(sections.failed_steps, MAX_FAILED_STEPS_CHARS);
    let mut files_context = truncate_for_prompt(sections.files_context, MAX_FILES_CONTEXT_CHARS);
    let mut ref_context = truncate_for_prompt(sections.ref_context, MAX_REF_CONTEXT_CHARS);
    let mut culprit_contents =
        truncate_for_prompt(sections.culprit_contents, MAX_CULPRIT_CONTENT_CHARS);

    let mut prompt = assemble_repair_prompt(&RepairPromptSections {
        goal: sections.goal,
        attempt_bundle: &attempt_bundle,
        combined_hints: &combined_hints,
        mutation_note: &mutation_note,
        failed_steps: &failed_steps,
        files_context: &files_context,
        ref_context: &ref_context,
        culprit_contents: &culprit_contents,
    });

    let mut overflow = prompt
        .chars()
        .count()
        .saturating_sub(MAX_REPAIR_PROMPT_CHARS);
    if overflow == 0 {
        return prompt;
    }

    shrink_section_to_fit(&mut ref_context, &mut overflow, 0);
    shrink_section_to_fit(&mut culprit_contents, &mut overflow, 0);
    shrink_section_to_fit(&mut combined_hints, &mut overflow, 300);
    shrink_section_to_fit(&mut mutation_note, &mut overflow, 0);
    shrink_section_to_fit(&mut failed_steps, &mut overflow, 800);
    shrink_section_to_fit(&mut files_context, &mut overflow, 1_500);
    shrink_section_to_fit(&mut attempt_bundle, &mut overflow, 500);

    prompt = assemble_repair_prompt(&RepairPromptSections {
        goal: sections.goal,
        attempt_bundle: &attempt_bundle,
        combined_hints: &combined_hints,
        mutation_note: &mutation_note,
        failed_steps: &failed_steps,
        files_context: &files_context,
        ref_context: &ref_context,
        culprit_contents: &culprit_contents,
    });

    prompt
}

//
// REPAIRING
//

pub async fn do_repairing(
    ctx: &mut ExecutionContext,
    llm: &dyn LLMProvider,
    goal: &str,
    workspace: &Path,
    config: &ContextConfig,
    repair_fingerprints: &mut Vec<u64>,
    error_history: &mut Vec<String>,
) -> Result<(Vec<Cmd>, AgentState)> {
    let all_err = ctx
        .failed_steps
        .iter()
        .map(|f| f.stderr.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let failure_kind = crate::failure::FailureKind::classify(&all_err);
    ctx.current_failure_kind = Some(failure_kind.clone());

    // InfraError Handling
    if failure_kind == FailureKind::InfraError {
        let infra_retries = ctx.repair_attempts;
        if infra_retries >= 3 {
            return Ok((
                Vec::new(),
                AgentState::Failed(
                    "Infrastructure failure: network/API unavailable after 3 retries.".into(),
                ),
            ));
        }
        let wait = [15u64, 45, 120][infra_retries as usize];
        println!(
            "\n  Infra error  retry {}/3 in {}s (no LLM call)...",
            infra_retries + 1,
            wait
        );
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        ctx.repair_attempts += 1;
        return Ok((Vec::new(), AgentState::Executing));
    }

    ctx.repair_attempts += 1;

    let mut dynamic_max_repairs = ctx.max_repairs;
    let goal_lower = goal.to_lowercase();
    if goal_lower.contains("typescript")
        || goal_lower.contains("node.js")
        || goal_lower.contains("jest")
        || goal_lower.contains("http server")
        || goal_lower.contains("httptest")
        || (goal_lower.contains("go") && goal_lower.contains("http"))
    {
        dynamic_max_repairs = dynamic_max_repairs.max(5);
    }

    let repair_limit = match failure_kind {
        FailureKind::PatchError => dynamic_max_repairs,
        FailureKind::InfraError => 0,
        _ => dynamic_max_repairs,
    };
    if ctx.repair_attempts > repair_limit {
        let diag_report = crate::diagnostic::analyze(&all_err);
        let diagnostic = if diag_report.is_empty() {
            "Unknown stubborn error.".to_string()
        } else {
            diag_report.as_prompt_fragment()
        };

        let msg = format!(
            "Max repair attempts ({}) reached.\nDiagnostic: {}",
            repair_limit, diagnostic
        );
        if ctx.bench_mode || std::env::var("SEL_BENCH_MODE").is_ok() {
            return Ok((Vec::new(), AgentState::Failed(msg)));
        }
        return Ok((Vec::new(), AgentState::WaitingForUserInput(msg)));
    }

    let elapsed = ctx
        .start_time
        .map(|s| s.elapsed().as_secs_f32())
        .unwrap_or(0.0);
    eprintln!(
        "\n[{:.1}s]  Repairing (Attempt {}/{})...",
        elapsed, ctx.repair_attempts, repair_limit
    );

    // Pre-Repair Checklist v7.6
    let mut fix_plan = Vec::new();
    if let crate::decision::ChecklistResult::Handled =
        crate::decision::pre_repair_checklist(&mut fix_plan, ctx, workspace)
    {
        if fix_plan.is_empty() {
            println!("   ⚠️  Checklist returned Handled but produced no commands  falling through to LLM");
        } else {
            println!("   🔧 Checklist Fix: applied deterministic repair");
            return Ok((fix_plan, AgentState::Executing));
        }
    }

    // QuickFix v7.5
    if let Some(fix) = crate::memory::quick_fix(&all_err) {
        match fix {
            crate::memory::QuickFix::InstallPackage { command } => {
                println!("   ⚡ QuickFix: {}", command);
                // We return this as a plan to execute
                return Ok((vec![Cmd::Run { command }], AgentState::Executing));
            }
            crate::memory::QuickFix::AddGoImport { symbol } => {
                println!("   ⚡ QuickFix Go import: {}", symbol);
                let main_go = workspace.join("main.go");
                if main_go.exists() {
                    let content = std::fs::read_to_string(&main_go).unwrap_or_default();
                    if !content.contains("import") {
                        let search = "package main\n".to_string();
                        if content.contains(&search) {
                            return Ok((
                                vec![
                                    Cmd::PatchFile {
                                        path: "main.go".to_string(),
                                        search: search.clone(),
                                        replace: format!("package main\n\nimport \"{}\"\n", symbol),
                                    },
                                    Cmd::RunTests {
                                        target: "go test".to_string(),
                                    },
                                ],
                                AgentState::Executing,
                            ));
                        }
                    } else if content.contains("import (") {
                        let search = "import (".to_string();
                        if content.contains(&search) {
                            return Ok((
                                vec![
                                    Cmd::PatchFile {
                                        path: "main.go".to_string(),
                                        search: search.clone(),
                                        replace: format!("import (\n\t\"{}\"", symbol),
                                    },
                                    Cmd::RunTests {
                                        target: "go test".to_string(),
                                    },
                                ],
                                AgentState::Executing,
                            ));
                        }
                    }
                }
                // Fallback to LLM if we can't safely auto-patch
            }
        }
    }

    //  Structured Repair Memory v8.0 (Escalating Strategy)
    let display_limit = repair_limit;

    let fingerprint = error_fingerprint(&all_err);
    let previous_same_error_streak =
        trailing_same_fingerprint_count(repair_fingerprints, fingerprint);
    let same_error_streak = previous_same_error_streak + 1;
    repair_fingerprints.push(fingerprint);
    if repair_fingerprints.len() > 32 {
        let overflow = repair_fingerprints.len() - 32;
        repair_fingerprints.drain(0..overflow);
    }
    if same_error_streak >= 2 {
        println!(
            "   🔄 Repair loop escalation: same error streak = {}",
            same_error_streak
        );
    }

    let constitution_violation_count = recent_constitution_violation_count(error_history, &all_err);

    let repair_ctx = crate::repair_strategy::RepairCtx::build(workspace, goal, error_history);
    let pattern_language = crate::pattern_library::infer_language_from_workspace(workspace);
    let pattern_lib = crate::pattern_library::PatternLibrary::load();
    let matched_pattern = pattern_lib.lookup(pattern_language, &all_err);
    let mut effective_route = matched_pattern
        .as_ref()
        .map(|p| p.route.clone())
        .unwrap_or_else(|| crate::pattern_library::infer_route_from_stderr(&all_err));

    if constitution_violation_count >= 2 {
        println!("   🚫 Repeated constitution violation detected  forcing ForceSourceOnly route.");
        effective_route = crate::pattern_library::RepairRoute::ForceSourceOnly;
    }

    let attempt_note = format!(
        "ATTEMPT {}/{}:\n{}",
        ctx.repair_attempts,
        display_limit,
        crate::repair_strategy::build_prompt(
            ctx.repair_attempts,
            &all_err,
            &repair_ctx,
            matched_pattern,
            &effective_route,
        )
    );

    let loop_warning = if same_error_streak >= 3 {
        format!(
            "\n\nLOOP ESCALATION: The same error signature repeated {} times. You MUST choose a materially different repair strategy. Do not repeat the previous patch. Rewrite the smallest failing implementation unit if needed.",
            same_error_streak
        )
    } else if same_error_streak == 2 {
        "\n\nWARNING: The same error repeated twice. Do not repeat the same patch. Try a materially different fix strategy.".to_string()
    } else {
        String::new()
    };

    error_history.push(all_err.chars().take(800).collect());

    // v5.8: Failure Memory hints
    let memory_hint = crate::memory::FailureMemory::load().get_hints(
        &format!("{:?}", failure_kind),
        &all_err.chars().take(80).collect::<String>(),
    );

    // v7.8: Diagnostic Engine hints
    let diag_report = crate::diagnostic::analyze(&all_err);
    let diagnostic_hint = if diag_report.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", diag_report.as_prompt_fragment())
    };

    let go_http_router_hint = if (all_err.contains("404 page not found")
        || all_err.contains("got 404")
        || all_err.contains("Expected status code 200, got 404")
        || all_err.contains("Expected status code 400, got 404"))
        && workspace.join("main.go").exists()
        && workspace.join("main_test.go").exists()
    {
        let body_check = if all_err.contains("Expected status code 400, got 404") {
            r#"

ADDITIONAL: The POST /echo handler must explicitly check for empty body and return 400.
Suggested pattern:
  body, _ := io.ReadAll(r.Body)
  if len(bytes.TrimSpace(body)) == 0 {
      w.WriteHeader(http.StatusBadRequest)
      return
  }
  w.Header().Set("Content-Type", "text/plain")
  _, _ = w.Write(body)
"#
            .to_string()
        } else {
            String::new()
        };

        format!(
            r#"

[go/http-router] Tests are returning 404. In Go, routes registered only inside `main()` are not reliably available to `httptest.NewServer(...)`-based tests. Extract route registration into `func setupRouter() http.Handler`, create a new `http.ServeMux` there, register all handlers there, return the mux, and call `setupRouter()` from both `main()` and tests. Do NOT rely on handlers being registered only by `main()` or only on `http.DefaultServeMux` side effects.{}"#,
            body_check
        )
    } else {
        String::new()
    };

    let rust_arc_move_hint = if (all_err.contains("thread::spawn")
        || all_err.contains("std::thread")
        || all_err.contains("Arc"))
        && (all_err.contains("use of moved value")
            || all_err.contains("borrow of moved value")
            || all_err.contains("value borrowed here after move")
            || all_err.contains("does not implement `Copy`")
            || all_err.contains("does not implement Copy"))
    {
        "\n\n[rust/ownership-arc] You are fixing a Rust ownership error involving Arc and move closures. For each thread::spawn(move || ...), create fresh clones BEFORE the closure, e.g. `let data1_for_t1 = Arc::clone(&data1); let data2_for_t1 = Arc::clone(&data2);` then use those clones INSIDE the closure. Do not call Arc::clone(&data1) after `data1` has already been moved into a previous closure. Do not move a MutexGuard into another variable and then reuse the old binding.".to_string()
    } else {
        String::new()
    };

    let missing_tests_hint = if failure_kind == FailureKind::MissingTests {
        "\n\n[rust/zero-tests] `cargo test` ran 0 tests. Do NOT just re-run tests. Add a REAL `#[cfg(test)] mod tests` block to the source file under test (for Rust binaries this is often `src/main.rs`) and include at least one `#[test]` that exercises the required behavior from the goal.".to_string()
    } else {
        String::new()
    };

    let combined_hints = format!(
        "{}{}{}{}{}",
        memory_hint, diagnostic_hint, go_http_router_hint, rust_arc_move_hint, missing_tests_hint
    );

    let culprit_files: Vec<String> = ctx
        .failed_steps
        .iter()
        .filter_map(|step| step.culprit_file.clone())
        .collect();

    let force_include: Vec<std::path::PathBuf> = culprit_files
        .iter()
        .map(|rel| workspace.join(rel))
        .filter(|path| path.exists())
        .collect();

    let dependency_graph = ctx
        .cached_dependency_graph_for(workspace)
        .cloned()
        .or_else(|| {
            let graph = crate::dependency_graph::builder::build_for_workspace(workspace);
            ctx.cache_dependency_graph(workspace, graph.clone());
            Some(graph)
        });

    let (smart_files_context, repair_budget) = crate::context::builder::build_repair_context_block(
        workspace,
        &crate::context::builder::RepairContext {
            stderr: all_err.clone(),
            recent_edits: ctx.recent_edits.clone(),
            max_tokens: crate::context::builder::MAX_REPAIR_TOKENS,
            force_include,
            culprit_files: culprit_files.clone(),
            context_config: Some(config.clone()),
            workspace: Some(workspace.to_path_buf()),
            dependency_graph,
        },
    );

    ctx.context_tokens_total += repair_budget.tokens_after as u64;
    ctx.context_tokens_before_total += repair_budget.tokens_before as u64;
    ctx.context_files_total += repair_budget.selected_files as u64;
    ctx.context_budget_samples += 1;
    ctx.force_include_dropped_count += repair_budget.force_include_dropped.len() as u64;

    let files_context = if smart_files_context.trim().is_empty() {
        crate::decision::build_workspace_context(workspace)
    } else {
        smart_files_context
    };

    // v8.1: File Content in every Repair Prompt for culprit files
    let mut culprit_contents = String::new();
    let mut seen_culprits = std::collections::HashSet::new();
    for step in &ctx.failed_steps {
        if let Some(ref culprit) = step.culprit_file {
            if seen_culprits.insert(culprit.clone()) {
                let path = workspace.join(culprit);
                if path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        culprit_contents.push_str(&format!(
                            "\n\n CURRENT CULPRIT FILE CONTENT: {} (use EXACT text from here for patch_file search)\n```\n{}\n```\n",
                            culprit, content
                        ));
                    }
                }
            }
        }
    }

    let mutation_note = if let Some(ref mctx) = ctx.last_mutation_context {
        let is_loop = repair_fingerprints.contains(&fingerprint);
        let loop_msg = if is_loop {
            "CRITICAL: You are trapped in a repair loop. DO NOT modify the implementation code! Write a STRICTER test case that specifically FAILS when this exact mutation is applied."
        } else {
            "Add tests to kill it."
        };
        format!("\n\n MUTATION SURVIVED:\n{}\n{}", mctx.surviving, loop_msg)
    } else {
        String::new()
    };

    let ref_context = crate::decision::build_ref_context(config);

    let attempt_bundle = format!("{}{}", attempt_note, loop_warning);

    let prompt_sections = RepairPromptSections {
        goal,
        attempt_bundle: &attempt_bundle,
        combined_hints: &combined_hints,
        mutation_note: &mutation_note,
        failed_steps: &all_err,
        files_context: &files_context,
        ref_context: &ref_context,
        culprit_contents: &culprit_contents,
    };

    let prompt = build_budgeted_repair_prompt(&prompt_sections);

    match plan_with_resilience(llm, prompt, ctx.bench_mode).await {
        Ok(mut commands) => {
            for cmd in &mut commands {
                if let Cmd::Run { command } = cmd {
                    let trimmed = command.trim().to_string();
                    let is_test_command = trimmed.starts_with("venv/bin/pytest")
                        || trimmed == "pytest"
                        || trimmed.starts_with("pytest ")
                        || trimmed.starts_with("cargo test")
                        || trimmed.starts_with("go test")
                        || trimmed == "npm test"
                        || trimmed.starts_with("npm test ")
                        || trimmed.starts_with("npx jest");

                    if is_test_command {
                        println!(
                            "   ⚡ Repair plan normalize: run -> run_tests ({})",
                            trimmed
                        );
                        *cmd = Cmd::RunTests { target: trimmed };
                    }
                }
            }

            normalize_repair_plan_order(&mut commands);
            drop_redundant_go_mod_init_commands(workspace, &mut commands);

            let mut initial_issues = crate::decision::validate_plan_integrity(&commands);
            initial_issues.extend(crate::decision::validate_rust_bootstrap_plan(
                workspace, &commands,
            ));
            if !initial_issues.is_empty() {
                println!("   ⚠️  Repair plan issues detected:");
                for issue in &initial_issues {
                    println!("       {}", issue);
                }

                let mut last_write_index = std::collections::HashMap::new();
                for (idx, cmd) in commands.iter().enumerate() {
                    if let Cmd::WriteFile { path, .. } = cmd {
                        last_write_index.insert(path.clone(), idx);
                    }
                }

                commands = commands
                    .into_iter()
                    .enumerate()
                    .filter_map(|(idx, cmd)| match &cmd {
                        Cmd::WriteFile { path, .. } => {
                            if last_write_index.get(path) == Some(&idx) {
                                Some(cmd)
                            } else {
                                println!(
                                    "   🗑️  Dropped duplicate repair write_file for '{}' (keeping last one)",
                                    path
                                );
                                None
                            }
                        }
                        _ => Some(cmd),
                    })
                    .collect();

                let remaining_issues = crate::decision::validate_plan_integrity(&commands);
                if !remaining_issues.is_empty() {
                    let reason = remaining_issues.join("\n");
                    return Ok((Vec::new(), AgentState::Failed(reason)));
                }
            }

            if let Some(clippy_cmd) = preserved_validator_command(ctx) {
                let already_has_clippy = commands.iter().any(|cmd| {
                    matches!(cmd, Cmd::Run { command } if command.trim_start().starts_with("cargo clippy"))
                });

                if !already_has_clippy {
                    println!("   ⚡ Repair plan preserve validator: {}", clippy_cmd);
                    insert_before_done(
                        &mut commands,
                        Cmd::Run {
                            command: clippy_cmd,
                        },
                    );
                }
            }

            println!("   🔧 Repair plan: {} commands", commands.len());
            Ok((commands, AgentState::Executing))
        }
        Err(e) => Ok((Vec::new(), AgentState::Failed(e))),
    }
}

fn normalize_repair_plan_order(commands: &mut Vec<Cmd>) {
    let mut done_cmd = None;
    if matches!(commands.last(), Some(Cmd::Done { .. })) {
        done_cmd = commands.pop();
    }

    let mut others = Vec::new();
    let mut last_run_tests = None;

    for cmd in commands.drain(..) {
        if cmd.is_run_tests() {
            last_run_tests = Some(cmd);
        } else {
            others.push(cmd);
        }
    }

    if let Some(run_tests) = last_run_tests {
        others.push(run_tests);
    }

    if let Some(done) = done_cmd {
        others.push(done);
    }

    *commands = others;
}

fn drop_redundant_go_mod_init_commands(workspace: &Path, commands: &mut Vec<Cmd>) {
    if !workspace.join("go.mod").exists() {
        return;
    }

    let before = commands.len();
    commands.retain(|cmd| match cmd {
        Cmd::Run { command } => {
            let lc = command.trim().to_lowercase();
            !(lc == "go mod init"
                || lc.starts_with("go mod init ")
                || lc.starts_with("go mod init\t"))
        }
        _ => true,
    });

    let dropped = before.saturating_sub(commands.len());
    if dropped > 0 {
        println!(
            "   ⚡ Repair plan sanitize: dropped {} redundant `go mod init` command(s)",
            dropped
        );
    }
}

fn preserved_validator_command(ctx: &crate::types::ExecutionContext) -> Option<String> {
    ctx.failed_steps
        .iter()
        .chain(ctx.last_failed_steps.iter())
        .find_map(|step| {
            step.label
                .trim()
                .strip_prefix("run: ")
                .map(str::trim)
                .filter(|cmd| cmd.starts_with("cargo clippy"))
                .map(|cmd| cmd.to_string())
        })
}

fn insert_before_done(commands: &mut Vec<Cmd>, cmd: Cmd) {
    let done_pos = commands
        .iter()
        .position(|c| matches!(c, Cmd::Done { .. }))
        .unwrap_or(commands.len());
    commands.insert(done_pos, cmd);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_fingerprint_is_stable() {
        let a = error_fingerprint("same error");
        let b = error_fingerprint("same error");
        let c = error_fingerprint("different error");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_trailing_same_fingerprint_count_counts_tail_only() {
        let a = error_fingerprint("a");
        let b = error_fingerprint("b");
        let history = vec![a, b, b, b];
        assert_eq!(trailing_same_fingerprint_count(&history, b), 3);
        assert_eq!(trailing_same_fingerprint_count(&history, a), 0);
    }

    #[test]
    fn test_recent_constitution_violation_count_includes_current_error() {
        let history = vec![
            "some other error".to_string(),
            "CONSTITUTION_VIOLATION:no-modify-tests".to_string(),
        ];
        let count =
            recent_constitution_violation_count(&history, "CONSTITUTION_VIOLATION:no-modify-tests");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_edited_path_from_cmd_returns_file_ops_only() {
        assert_eq!(
            edited_path_from_cmd(&Cmd::WriteFile {
                path: "src/lib.rs".into(),
                content: "x".into(),
            }),
            Some("src/lib.rs")
        );
        assert_eq!(
            edited_path_from_cmd(&Cmd::PatchFile {
                path: "src/main.rs".into(),
                search: "a".into(),
                replace: "b".into(),
            }),
            Some("src/main.rs")
        );
        assert_eq!(
            edited_path_from_cmd(&Cmd::RunTests {
                target: "cargo test".into(),
            }),
            None
        );
    }

    #[test]
    fn test_truncate_for_prompt_respects_limit() {
        let out = truncate_for_prompt(&"x".repeat(20), 8);
        assert_eq!(out.chars().count(), 8);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn test_build_budgeted_repair_prompt_respects_global_cap() {
        let long = "x".repeat(60_000);
        let sections = RepairPromptSections {
            goal: "goal",
            attempt_bundle: &long,
            combined_hints: &long,
            mutation_note: &long,
            failed_steps: &long,
            files_context: &long,
            ref_context: &long,
            culprit_contents: &long,
        };

        let prompt = build_budgeted_repair_prompt(&sections);
        assert!(prompt.chars().count() <= MAX_REPAIR_PROMPT_CHARS);
        assert!(prompt.contains("Goal: goal"));
        assert!(prompt.contains("Fix ALL issues."));
    }

    #[test]
    fn test_plan_risk_feedback_respects_toggle() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn a() {}
",
        )
        .unwrap();

        let risky_plan = vec![Cmd::WriteFile {
            path: "src/lib.rs".into(),
            content: "pub fn b() {}
"
            .into(),
        }];

        let enabled_feedback = plan_risk_feedback_with_flag(dir.path(), &risky_plan, true);
        let disabled_feedback = plan_risk_feedback_with_flag(dir.path(), &risky_plan, false);

        assert!(!enabled_feedback.is_empty());
        assert!(disabled_feedback.is_empty());
    }

    #[test]
    fn test_plan_risk_feedback_keeps_safe_plan_clear() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn a() {}
",
        )
        .unwrap();

        let safe_plan = vec![
            Cmd::PatchFile {
                path: "src/lib.rs".into(),
                search: "a".into(),
                replace: "b".into(),
            },
            Cmd::RunTests {
                target: "cargo test".into(),
            },
            Cmd::Done {
                message: "ok".into(),
            },
        ];

        let feedback = plan_risk_feedback_with_flag(dir.path(), &safe_plan, true);
        assert!(feedback.is_empty());
    }

    #[test]
    fn test_record_plan_risk_telemetry_sets_fields() {
        let mut ctx = ExecutionContext::new(3);
        let commands = vec![
            Cmd::WriteFile {
                path: "src/lib.rs".into(),
                content: "pub fn b() {}\n".into(),
            },
            Cmd::RunTests {
                target: "cargo test".into(),
            },
        ];
        let issues = vec![
            "PLAN RISK: write_file targets existing source file 'src/lib.rs'".to_string(),
            "PLAN RISK: plan has 8 commands".to_string(),
        ];

        record_plan_risk_telemetry(&mut ctx, &commands, &issues);

        assert!(ctx.plan_risk_triggered);
        assert_eq!(ctx.plan_risk_reasons, issues);
        assert_eq!(ctx.commands_before_replan, 2);
    }

    #[test]
    fn test_record_plan_risk_telemetry_ignores_empty_issues() {
        let mut ctx = ExecutionContext::new(3);
        let commands = vec![Cmd::Done {
            message: "ok".into(),
        }];

        record_plan_risk_telemetry(&mut ctx, &commands, &[]);

        assert!(!ctx.plan_risk_triggered);
        assert!(ctx.plan_risk_reasons.is_empty());
        assert_eq!(ctx.commands_before_replan, 0);
    }

    #[test]
    fn test_record_plan_risk_telemetry_updates_reasons_but_keeps_first_command_count() {
        let mut ctx = ExecutionContext::new(3);

        let first_commands = vec![
            Cmd::WriteFile {
                path: "src/lib.rs".into(),
                content: "pub fn a() {}\n".into(),
            },
            Cmd::RunTests {
                target: "cargo test".into(),
            },
        ];
        let first_issues =
            vec!["PLAN RISK: write_file targets existing source file 'src/lib.rs'".to_string()];

        record_plan_risk_telemetry(&mut ctx, &first_commands, &first_issues);

        let second_commands = vec![
            Cmd::WriteFile {
                path: "src/lib.rs".into(),
                content: "pub fn b() {}\n".into(),
            },
            Cmd::RunTests {
                target: "cargo test".into(),
            },
            Cmd::Done {
                message: "ok".into(),
            },
        ];
        let second_issues = vec![
            "PLAN RISK: delete_file on 'src/lib.rs' is destructive and should be avoided unless strictly necessary.".to_string(),
        ];

        record_plan_risk_telemetry(&mut ctx, &second_commands, &second_issues);

        assert!(ctx.plan_risk_triggered);
        assert_eq!(ctx.plan_risk_reasons, second_issues);
        assert_eq!(ctx.commands_before_replan, 2);
    }
}
