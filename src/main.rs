#![allow(dead_code)]
mod bench_cases;
mod bench_compile;
mod bench_realworld;
mod bench_suite;
mod failure;
pub mod llm;
mod trajectory_index;
mod workspace_oracle;
// src/main.rs  SEL Agent v8.5.0
mod agent;
mod chunker;
mod constraint_engine;
mod context;
mod environment;
mod evaluator;
mod executor;
mod goal_parser;

pub mod bench_sel;
pub mod bench_swe;
pub mod cache;
mod constitution;
pub mod cost;
mod decision;
pub mod diagnostic;
mod manifest;
mod memory;
mod protocol;
pub mod provider_state;
mod repair_strategy;
mod scaffold_engine;
mod snapshot;
mod state_handlers;
mod types;

pub mod commands;

use anyhow::Result;
use clap::Parser;
use commands::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let api_key = std::env::var("SEL_API_KEY")
        .or_else(|_| std::env::var("OPENROUTER_API_KEY"))
        .or_else(|_| std::env::var("GEMINI_API_KEY"))
        .or_else(|_| std::env::var("GROQ_API_KEY"))
        .unwrap_or_default();

    match cli.command {
        Commands::Health => {
            commands::run_health(&api_key).await?;
        }
        Commands::Bench {
            suite,
            max_repairs,
            iterations,
            focus,
            record,
            replay,
            quick,
            delay,
            skip_recorded,
            rerecord,
        } => {
            let failed = if quick {
                commands::run_quick_bench(&api_key, &suite, max_repairs, iterations, &focus, delay)
                    .await?
            } else {
                commands::run_bench(
                    &api_key,
                    &suite,
                    max_repairs,
                    iterations,
                    &focus,
                    record,
                    replay,
                    delay,
                    skip_recorded,
                    rerecord,
                )
                .await?
            };
            if !failed.is_empty() {
                std::process::exit(1);
            }
        }
        Commands::Stress {
            max_repairs,
            cases,
            delay,
            record,
            replay,
            rerecord,
        } => {
            commands::run_stress(
                &api_key,
                max_repairs,
                cases,
                delay,
                record,
                replay,
                rerecord,
            )
            .await?;
        }
        Commands::Scan { workspace, json } => {
            commands::cmd_scan(&workspace, json);
        }
        Commands::Compare {
            models,
            suite,
            max_repairs,
        } => {
            commands::run_compare(&models, &suite, max_repairs).await?;
        }
        Commands::Plan {
            workspace,
            plan,
            max_repairs,
        } => {
            commands::run_plan(&api_key, &workspace, &plan, max_repairs).await?;
        }
        Commands::BenchSwe {
            lang,
            focus,
            max_repairs,
            delay,
            record,
            replay,
            rerecord,
        } => {
            let key = api_key;
            commands::bench::run_bench_swe_cmd(
                &key,
                &lang,
                focus.as_deref(),
                max_repairs,
                delay,
                record,
                replay,
                rerecord,
            )
            .await?;
        }
        Commands::BenchRealWorld {
            tier,
            max_repairs,
            record,
            replay,
            rerecord,
            delay,
            skip_recorded,
            focus,
        } => {
            crate::bench_realworld::run_bench_realworld(
                &api_key,
                tier,
                max_repairs,
                record,
                replay,
                rerecord,
                delay,
                skip_recorded,
                &focus,
            )
            .await?;
        }
        Commands::BenchSelV11 {
            focus,
            max_repairs,
            delay,
            include_system,
            record,
            replay,
            rerecord,
        } => {
            crate::bench_sel::run_bench_sel_v11(
                &api_key,
                focus.as_deref(),
                max_repairs,
                delay,
                include_system,
                record,
                replay,
                rerecord,
            )
            .await?;
        }
        Commands::BenchSel {
            focus,
            max_repairs,
            delay,
            record,
            replay,
            rerecord,
        } => {
            crate::bench_sel::run_bench_sel(
                &api_key,
                focus.as_deref(),
                max_repairs,
                delay,
                record,
                replay,
                rerecord,
            )
            .await?;
        }
        Commands::ResetProviders => {
            let path = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|pp| pp.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".sel-agent")
                .join("provider_state.json");
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("\n Provider cache cleared  all keys are now active.");
            } else {
                println!("\n Provider cache was already clean  nothing to reset.");
            }
            println!("\n Current key counts:");
            let providers = [
                ("GROQ_API_KEY", "Groq"),
                ("CEREBRAS_API_KEY", "Cerebras"),
                ("GEMINI_API_KEY", "Gemini"),
                ("OPENROUTER_API_KEY", "OpenRouter"),
                ("GITHUB_TOKEN", "GitHub"),
                ("SEL_API_KEY", "SEL"),
            ];
            for (env_key, label) in &providers {
                let pool = crate::llm::key_pool::KeyPool::from_env(env_key);
                let n = pool.keys.len();
                if n > 0 {
                    println!("     {:<12} {} key(s) available", label, n);
                } else {
                    println!("      {:<12} no keys found (set {} in env)", label, env_key);
                }
            }
            println!();
        }
        Commands::Run {
            workspace,
            goal,
            max_repairs,
            dry_run,
            ref_file,
            focus,
            record,
            replay,
            rerecord,
        } => {
            use crate::llm::LLMProvider;

            println!("\n");
            println!("   SEL Agent v8.5.0  State Machine Engine   ");
            println!();
            println!("\n Goal: \"{}\"", goal);
            {
                let engine = crate::llm::live::LiveProvider::from_env();
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
                println!("   Mode:          DRY RUN\n");
                let llm = llm::live::LiveProvider::from_env();
                let prompt = format!("Goal: {}\n\nProvide the complete execution plan.", goal);
                let req = llm::LLMRequest {
                    system: llm::get_system_prompt(false),
                    messages: vec![types::Message::user(prompt)],
                    temperature: 0.0,
                    seed: Some(42),
                    model: "default".into(),
                };
                match llm.complete(req).await {
                    Ok(response) => match protocol::parse(&response.content) {
                        Ok(plan) => {
                            println!(" Plan preview ({} commands):\n", plan.commands.len());
                            for (i, cmd) in plan.commands.iter().enumerate() {
                                println!("  [{}/{}] {}", i + 1, plan.commands.len(), cmd.label());
                            }
                            println!("\n DRY RUN complete.");
                        }
                        Err(e) => println!(" Plan parse error: {}", e),
                    },
                    Err(e) => println!(" LLM error: {}", e),
                }
                return Ok(());
            }

            std::fs::create_dir_all(&workspace)?;

            let traj_dir = if record || replay {
                let slug = goal
                    .to_lowercase()
                    .chars()
                    .filter(|c| c.is_alphanumeric() || c.is_whitespace())
                    .collect::<String>()
                    .replace(" ", "_");
                let slug = if slug.chars().count() > 30 {
                    slug.chars().take(30).collect::<String>()
                } else {
                    slug
                };
                let cache = crate::cache::PersistentCache::new()?;
                let dir = cache.trajectories_dir().join(format!("run_{}", slug));
                Some(dir)
            } else {
                None
            };

            let live = crate::llm::live::LiveProvider::from_env();
            let provider: Box<dyn crate::llm::LLMProvider> = if replay {
                let dir = traj_dir
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Trajectory dir missing"))?;
                Box::new(crate::llm::replay::ReplayProvider::new(&dir))
            } else if record {
                let dir = traj_dir
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Trajectory dir missing"))?;
                Box::new(crate::llm::record::RecorderProvider::new(
                    Box::new(live),
                    &dir,
                ))
            } else {
                Box::new(live)
            };

            let ref_file_expanded = ref_file.as_ref().map(|p| {
                let s = p.to_string_lossy();
                if let Some(s_stripped) = s.strip_prefix("~/") {
                    if let Ok(home) = std::env::var("HOME") {
                        return std::path::PathBuf::from(format!("{}/{}", home, s_stripped));
                    }
                }
                p.clone()
            });
            let ctx_config = types::ContextConfig {
                ref_file: ref_file_expanded,
                focus_paths: focus.clone(),
                ..Default::default()
            };

            let mut ag = agent::Agent::new_with_model(
                String::new(),
                String::new(),
                workspace.clone(),
                goal.clone(),
                max_repairs,
                ctx_config.clone(),
                provider,
            );
            if std::env::var("SEL_BENCH_MODE").is_ok() {
                ag.bench_mode = true;
            }
            let run_res = ag.run().await;

            let is_ok = match run_res {
                Ok(_) => ag.is_success(),
                Err(_) => false,
            };

            if replay && !is_ok && rerecord {
                println!("     Run failed in replay mode! Auto-rerecording trajectory...");
                if let Some(ref dir) = traj_dir {
                    let live2 = crate::llm::live::LiveProvider::from_env();
                    let recorder = Box::new(crate::llm::record::RecorderProvider::new(
                        Box::new(live2),
                        dir,
                    ));
                    let mut heal_ag = agent::Agent::new_with_model(
                        String::new(),
                        String::new(),
                        workspace.clone(),
                        goal.clone(),
                        max_repairs,
                        ctx_config.clone(),
                        recorder,
                    );
                    heal_ag.bench_mode = ag.bench_mode;
                    let _ = heal_ag.run().await;
                    if heal_ag.is_success() {
                        println!("    Successfully auto-rerecorded.");
                    } else {
                        println!("    Auto-rerecord failed.");
                        return Err(anyhow::anyhow!("Run failed even after auto-rerecord"));
                    }
                }
            } else {
                run_res?;
            }
        }
    }
    Ok(())
}
