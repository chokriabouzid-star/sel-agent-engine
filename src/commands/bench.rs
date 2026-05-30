#![allow(clippy::too_many_arguments)]
use anyhow::Result;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};

use std::time::Duration;

use crate::types;

use crate::commands::health::{shorten_provider, ProviderStats};

#[allow(clippy::too_many_arguments)]
pub async fn run_bench(
    api_key: &str,
    suite: &str,
    max_repairs: u8,
    iterations: u8,
    focus: &[String],
    record: bool,
    replay: bool,
    delay: u64,
    skip_recorded: bool,
    rerecord: bool,
) -> Result<Vec<String>> {
    let cases = crate::bench_cases::suite_cases(suite);
    let cases: Vec<_> = cases
        .into_iter()
        .filter(|c| focus.is_empty() || focus.contains(&c.name.to_string()))
        .collect();

    // v5.7: integration suite
    // v7.1: compile suite
    if suite == "compile" {
        run_compile_bench(max_repairs).await?;
        return Ok(Vec::new());
    }
    if suite == "integration" {
        run_integration_bench("", max_repairs).await?;
        return Ok(Vec::new());
    }

    if cases.is_empty() {
        if !focus.is_empty() {
            println!(
                " ⚠️  No cases matched the focus filter '{:?}' in suite '{}'.",
                focus, suite
            );
        } else {
            println!(
                " ❌ Unknown suite '{}'. Use: python, go, node, rust, typescript, integration, all",
                suite
            );
        }
        return Ok(Vec::new());
    }

    println!("╔══════════════════════════════════════════╗");
    println!("║   SEL Bench v1.8 — suite: {:<14} ║", suite);
    println!("╚══════════════════════════════════════════╝\n");

    // v7.9.6: Create LiveProvider ONCE  shared across all tasks (KeyPool memory persists)
    // v8.0: Also create LiveProvider when rerecord=true, because we may need it to heal broken replays
    let shared_llm = if !replay || rerecord {
        Some(crate::llm::live::LiveProvider::from_env())
    } else {
        None
    };

    let total = cases.len();
    let total_runs = total * iterations as usize;

    if let Some(ref engine) = shared_llm {
        engine.print_info();
        crate::llm::preflight_quota_check(total_runs, engine);
    } else {
        println!(" 📡 Replay mode  offline (no API keys required)");
    }
    let mut passed = 0usize;
    let mut total_repairs = 0usize;
    let mut mutation_killed = 0u32;
    let mut mutation_total = 0u32;
    let mut failed_cases = Vec::new();
    let tmpdir = std::env::temp_dir();
    let mut provider_stats = ProviderStats::default();
    let bench_start_time = std::time::Instant::now();
    let mut auto_healed = Vec::new();

    for iter in 0..iterations {
        if iterations > 1 {
            println!("\n Iteration {}/{} ", iter + 1, iterations);
        }
        for (i, case) in cases.iter().enumerate() {
            let completed = iter as usize * total + i;
            if completed > 0 && delay > 0 {
                println!("   ⏳ Cooling down {}s before next task...", delay);
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }

            let name = &case.name;
            let goal = &case.goal;

            // v7.9.10: --skip-recorded feature
            // Trajectories are stored as 001.json, 002.json, etc. (not trajectory.json)
            if skip_recorded && record {
                let traj_base = std::env::current_dir()
                    .unwrap_or_default()
                    .join("fixtures")
                    .join("trajectories");
                let traj_dir = traj_base.join(name.replace(" ", "_"));
                let first_turn = traj_dir.join("001.json");
                if first_turn.exists() {
                    println!("   ⏭  Skipping '{}'  trajectory exists", name);
                    continue;
                }
            }

            let workspace = tmpdir.join(format!("sel-bench-{}-{}", iter, i));
            let _ = std::fs::remove_dir_all(&workspace);
            std::fs::create_dir_all(&workspace).ok();

            // v7.9.9 P1: Write scaffold files (broken code for bugfix tasks)
            if case.is_bugfix() {
                for (path, content) in &case.scaffold_files {
                    let file_path = workspace.join(path);
                    if let Some(parent) = file_path.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    std::fs::write(&file_path, content).ok();
                }
                println!(
                    "   🏗  Scaffold ready: {} ({} files)",
                    case.lang,
                    case.scaffold_files.len()
                );
            }

            let completed = iter as usize * total + i;
            let eta_str = if completed > 0 {
                let elapsed = bench_start_time.elapsed().as_secs_f32();
                let avg_secs = elapsed / completed as f32;
                let remaining = total_runs - completed;
                let eta = avg_secs * remaining as f32;
                format!("(avg: {:.1}s, ETA: ~{:.0}s)", avg_secs, eta)
            } else {
                "(ETA: calc...)".to_string()
            };

            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template(&format!(
                        "{{spinner:.cyan}}  [{:02}/{:02}] {}... {}",
                        completed + 1,
                        total_runs,
                        name,
                        eta_str
                    ))
                    .unwrap(),
            );
            pb.enable_steady_tick(Duration::from_millis(80));

            // v7.9.6: Unified trajectory path  always use fixtures/trajectories/
            let traj_base = std::env::current_dir()
                .unwrap_or_default()
                .join("fixtures")
                .join("trajectories");
            let llm: Box<dyn crate::llm::LLMProvider> = if replay {
                let replay_dir = traj_base.join(name.replace(" ", "_"));
                Box::new(crate::llm::replay::ReplayProvider::new(replay_dir))
            } else {
                // v7.9.6: clone_shared()  reuses same KeyPools (exhausted keys stay exhausted)
                let base_llm = Box::new(shared_llm.as_ref().unwrap().clone_shared());
                if record {
                    let record_dir = traj_base.join(name.replace(" ", "_"));
                    Box::new(crate::llm::record::RecorderProvider::new(
                        base_llm, record_dir,
                    ))
                } else {
                    base_llm
                }
            };

            let mut agent = crate::agent::Agent::new_with_model(
                api_key.to_string(),
                "default".to_string(),
                workspace.clone(),
                goal.to_string(),
                max_repairs,
                types::ContextConfig::default(),
                llm,
            );
            agent.bench_mode = true; // v7.9.8: skip EXPLAIN MODE
            let mut ok = agent.run().await.is_ok();
            pb.finish_and_clear();

            // v8.0: Auto-Heal (Rerecord) broken trajectories during offline replay
            if replay && !ok && rerecord {
                let err_reason = agent
                    .failed_reason()
                    .unwrap_or_else(|| "Unknown Error".to_string());
                println!(
                    "   ❌ Replay failed ({})! Auto-rerecording trajectory...",
                    err_reason
                );

                // 1. Clean workspace for fresh start
                let _ = std::fs::remove_dir_all(&workspace);
                std::fs::create_dir_all(&workspace).ok();
                if case.is_bugfix() {
                    for (path, content) in &case.scaffold_files {
                        let file_path = workspace.join(path);
                        if let Some(parent) = file_path.parent() {
                            std::fs::create_dir_all(parent).ok();
                        }
                        std::fs::write(&file_path, content).ok();
                    }
                }

                // 2. Clear old broken trajectory
                let record_dir = traj_base.join(name.replace(" ", "_"));
                let _ = std::fs::remove_dir_all(&record_dir);

                // 3. Setup LiveProvider with RecorderProvider
                let live = match shared_llm.as_ref() {
                    Some(l) => l,
                    None => {
                        println!("   ⚠️  Auto-rerecord skipped: no API keys configured. Re-run with API keys set.");
                        continue;
                    }
                };
                let base_llm = Box::new(live.clone_shared());
                let new_llm = Box::new(crate::llm::record::RecorderProvider::new(
                    base_llm, record_dir,
                ));

                // 4. Run agent in live/record mode
                let mut heal_agent = crate::agent::Agent::new_with_model(
                    api_key.to_string(),
                    "default".to_string(),
                    workspace.clone(),
                    goal.to_string(),
                    max_repairs,
                    types::ContextConfig::default(),
                    new_llm,
                );
                heal_agent.bench_mode = true;

                let heal_ok = heal_agent.run().await.is_ok();
                if heal_ok {
                    println!("   ✅ Successfully auto-rerecorded.");
                    ok = true; // We healed it!
                    auto_healed.push(format!("{}: {} -> RERECORDED", name, err_reason));
                    agent = heal_agent; // use the healed agent's stats
                } else {
                    println!("   ❌ Auto-rerecord failed.");
                }
            }

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
                "".to_string()
            };
            let model = agent.llm.get_stats().last_model;
            let model_str = if model.is_empty() {
                "none".to_string()
            } else {
                model
            };
            provider_stats.record(&model_str);
            let autofix = agent.ctx.autofix_count;
            println!(
                "   {} {:20} repairs:{} autofix:{} mutation:{} provider:{}",
                status, name, repairs, autofix, ms_str, model_str
            );
            if ok {
                passed += 1;
            } else {
                failed_cases.push(name.clone());
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

    println!("\n");
    println!("📊 SEL Bench Results                      ");
    println!();
    println!("  Suite:          {:<23}", suite);
    println!("  Iterations:     {:<23}", iterations);
    println!(
        "  Passed:         {:<23}",
        format!("{}/{}", passed, total_runs)
    );
    println!(
        "  Success Rate:   {:<23}",
        format!("{:.1}%", success_rate * 100.0)
    );
    let tested_percent = if total_runs > 0 {
        (mutation_total as f64 / total_runs as f64) * 100.0
    } else {
        0.0
    };
    let kill_rate = if mutation_total > 0 {
        (mutation_killed as f64 / mutation_total as f64) * 100.0
    } else {
        0.0
    };
    let full_coverage = if total_runs > 0 {
        (mutation_killed as f64 / total_runs as f64) * 100.0
    } else {
        0.0
    };

    println!("  Avg Repairs:    {:<23}", format!("{:.1}", avg_repairs));
    println!(
        "  Mutation Tested:{:<23}",
        format!(
            "{}/{} tasks ({:.0}%)",
            mutation_total, total_runs, tested_percent
        )
    );
    println!(
        "  Kill Rate:      {:<23}",
        format!("{:.0}% of tested", kill_rate)
    );
    println!("  Full Coverage:  {:<23}", format!("{:.0}%", full_coverage));
    println!("  Quality Index:  {:<23}", format!("{:.2}", quality));

    if !auto_healed.is_empty() {
        println!();
        println!("   🩹 Auto-Healed (rerecorded):            ");
        for h in &auto_healed {
            println!("    - {:<36}", h);
        }
    }

    println!();
    println!("🔗 Provider Usage:                     ");

    let mut sorted: Vec<_> = provider_stats.call_counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    for (provider, count) in &sorted {
        let short = shorten_provider(provider);
        println!("    {:<14}  {:>2} calls             ", short, count);
    }

    println!("    ");
    println!(
        "    {:<14}  {:>2} calls             ",
        "total",
        provider_stats.total()
    );
    println!("\n");

    // POST to Observatory
    let model =
        std::env::var("SEL_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string());
    let version = std::env::var("SEL_VERSION").unwrap_or_else(|_| "v8.5.0".to_string());
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

    Ok(failed_cases)
}

pub async fn run_stress(
    api_key: &str,
    max_repairs: u8,
    case_limit: usize,
    delay: u64,
    record: bool,
    replay: bool,
    rerecord: bool,
) -> Result<()> {
    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║   SEL Agent v8.5.0 🔥 Stress Test               ║".cyan()
    );
    println!(
        "{}",
        "╠══════════════════════════════════════════════════╣".cyan()
    );
    println!(
        "║  Cases: {:3}  Mode: {:<20}  ║",
        case_limit,
        if replay && rerecord {
            "REPLAY+RERECORD"
        } else if replay {
            "REPLAY"
        } else if record {
            "RECORD"
        } else {
            "LIVE"
        }
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════╝".cyan()
    );
    println!();

    let all_cases = crate::bench_cases::suite_cases("all");
    let cases: Vec<_> = all_cases.into_iter().take(case_limit).collect();

    let total = cases.len();
    let mut passed = 0usize;
    let mut healed = 0usize;
    let mut total_repairs = 0usize;
    let tmpdir = std::env::temp_dir();

    for (i, case) in cases.iter().enumerate() {
        let name = &case.name;
        let goal = &case.goal;
        let workspace = tmpdir.join(format!("sel-stress-{}", i));
        let _ = std::fs::remove_dir_all(&workspace);

        // trajectory directory
        let traj_dir = std::env::current_dir()
            .unwrap_or_default()
            .join("fixtures")
            .join("trajectories")
            .join(format!(
                "stress_{}",
                case.name.to_lowercase().replace(' ', "_")
            ));

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!(
                    "{{spinner:.yellow}}  [{:02}/{:02}] {}...",
                    i + 1,
                    total,
                    name
                ))
                .unwrap(),
        );
        pb.enable_steady_tick(Duration::from_millis(80));

        let provider: Box<dyn crate::llm::LLMProvider> = if replay {
            Box::new(crate::llm::replay::ReplayProvider::new(&traj_dir))
        } else {
            let live = crate::llm::live::LiveProvider::from_env();
            if record {
                let _ = std::fs::create_dir_all(&traj_dir);
                Box::new(crate::llm::record::RecorderProvider::new(
                    Box::new(live),
                    &traj_dir,
                ))
            } else {
                Box::new(live)
            }
        };

        let mut ag = crate::agent::Agent::new_with_model(
            api_key.to_string(),
            String::new(),
            workspace.clone(),
            goal.to_string(),
            max_repairs,
            types::ContextConfig::default(),
            provider,
        );
        ag.bench_mode = true;
        let result = ag.run().await;
        pb.finish_and_clear();

        let mut case_passed = false;
        if result.is_ok() && ag.is_success() {
            case_passed = true;
        }

        // rerecord on failure
        if !case_passed && rerecord {
            let _ = std::fs::remove_dir_all(&workspace);
            let _ = std::fs::create_dir_all(&workspace);
            let _ = std::fs::create_dir_all(&traj_dir);
            let live = crate::llm::live::LiveProvider::from_env();
            let rec = Box::new(crate::llm::record::RecorderProvider::new(
                Box::new(live),
                &traj_dir,
            ));
            let mut ag2 = crate::agent::Agent::new_with_model(
                api_key.to_string(),
                String::new(),
                workspace.clone(),
                goal.to_string(),
                max_repairs,
                types::ContextConfig::default(),
                rec,
            );
            ag2.bench_mode = true;
            let _ = ag2.run().await;
            if ag2.is_success() {
                case_passed = true;
                healed += 1;
            }
        }

        if case_passed {
            let repairs = ag.repair_count();
            total_repairs += repairs;
            println!(
                "    {} (repairs: {}{})",
                name.green(),
                repairs,
                if rerecord && healed > 0 { " 🩹" } else { "" }
            );
            passed += 1;
        } else {
            println!("    {}", name.red());
        }
        let _ = std::fs::remove_dir_all(&workspace);
        if i < total - 1 {
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
    }

    let avg = if passed > 0 {
        total_repairs as f64 / passed as f64
    } else {
        0.0
    };
    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "║  Stress Results: {}/{} passed | avg repairs: {:.1}  ║",
        passed, total, avg
    );
    if healed > 0 {
        println!(
            "║  Auto-healed: {}                                   ║",
            healed
        );
    }
    println!(
        "{}",
        "╚══════════════════════════════════════════════════╝".cyan()
    );
    println!();
    Ok(())
}

pub async fn run_integration_bench(api_key: &str, max_repairs: u8) -> Result<()> {
    println!("\n");
    println!("   SEL Integration Bench v5.7 🔗 🔗             ");
    println!("   Phase1: Build  Phase2: Patch          ");
    println!("\n");

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
        println!("\n Test {}/{}: {} ", i + 1, total, name);

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
                "   ❌ Phase1 FAILED (repairs: {})  skipping Phase2",
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
    println!("\n");
    println!("📊 Integration Bench Results               ");
    println!();
    println!(
        "  Tests:          {:<23}",
        format!("{} cases x 2 phases", total)
    );
    println!(
        "  Phase1 passed:  {:<23}",
        format!("{}/{}", phase1_passed, total)
    );
    println!(
        "  Phase2 passed:  {:<23}",
        format!("{}/{}", phase2_passed, total)
    );
    println!("  Full passed:    {:<23}", format!("{}/{}", passed, total));
    println!("  Avg Repairs:    {:<23}", format!("{:.1}", avg_repairs));
    println!("\n");

    Ok(())
}

pub async fn run_compile_bench(max_repairs: u8) -> Result<()> {
    use crate::bench_compile::{all_cases, check_result, setup_case};

    println!("\n");
    println!("   SEL Bench 🔨 suite: compile (v7.1)      ");
    println!("\n");

    let cases = all_cases();
    let total = cases.len();
    let mut passed = 0usize;
    let mut results: Vec<(String, String, usize, bool, String)> = Vec::new();
    let tmpdir = std::env::temp_dir();

    for (i, case) in cases.iter().enumerate() {
        let workspace = tmpdir.join(format!("sel-compile-{}", i));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).ok();

        // Setup:
        setup_case(case.name, &workspace);

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!(
                    "{{spinner:.cyan}}  [{}/{}] {}...",
                    i + 1,
                    total,
                    case.name
                ))
                .unwrap(),
        );
        pb.enable_steady_tick(Duration::from_millis(80));

        //  Agent  goal
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

        //
        let check = check_result(case.name, &workspace, ok, repairs, mutation);

        #[allow(clippy::if_same_then_else)]
        let status = if check.passed { "" } else { "" };
        let mut notes = Vec::new();
        if check.created_wrong_files {
            notes.push("wrong_files".to_string());
        }
        if !check.mutation_ok {
            notes.push("mutation_fail".to_string());
        }
        if repairs > 2 {
            notes.push(format!("repairs:{}", repairs));
        }
        let note_str = if notes.is_empty() {
            "ok".to_string()
        } else {
            notes.join(", ")
        };

        println!(
            "   {} {:<28} repairs:{}  {}",
            status, case.name, repairs, note_str
        );

        if check.passed {
            passed += 1;
        }
        results.push((
            case.name.to_string(),
            status.to_string(),
            repairs,
            check.passed,
            note_str,
        ));

        //
        let _ = std::fs::remove_dir_all(&workspace);
    }

    //
    let rate = passed as f64 / total as f64 * 100.0;
    println!("\n");
    println!("📊 Compile Bench Results                   ");
    println!();
    println!(
        "  Passed:  {}/{}  ({:.0}%)                   ",
        passed, total, rate
    );
    println!();

    for (name, status, repairs, _, note) in &results {
        println!(
            "  {} {:<22} r:{} {}",
            status,
            name,
            repairs,
            if note.len() > 15 { &note[..15] } else { note }
        );
    }

    println!();

    if rate >= 100.0 {
        println!("\n    v7.1  ");
    } else if rate >= 75.0 {
        println!("\n        ");
    } else {
        println!("\n    v7.1   ");
    }

    Ok(())
}

pub async fn run_quick_bench(
    api_key: &str,
    suite: &str,
    max_repairs: u8,
    iterations: u8,
    focus: &[String],
    delay: u64,
) -> Result<Vec<String>> {
    println!("\n ⚡ Quick Mode: Stage 1  Running Replay for '{}'", suite);
    let failed = run_bench(
        api_key,
        suite,
        max_repairs,
        iterations,
        focus,
        false,
        true,
        delay,
        false,
        false,
    )
    .await?;

    if failed.is_empty() {
        println!("\n ✅ Quick Mode: All cases passed via Replay. System is stable.");
        return Ok(Vec::new());
    }

    println!(
        "\n 🔧 Quick Mode: Stage 2  {} cases failed. Re-recording...",
        failed.len()
    );
    println!("   ❌ Failed: {:?}", failed);

    // Stage 3: Re-record failed ones only
    run_bench(
        api_key,
        suite,
        max_repairs,
        iterations,
        &failed,
        true,
        false,
        delay,
        false, // skip_recorded
        false, // rerecord
    )
    .await?;

    println!("\n 🔍 Quick Mode: Stage 3  Verifying failures via Replay...");
    let final_failed = run_bench(
        api_key,
        suite,
        max_repairs,
        iterations,
        &failed,
        false,
        true,
        delay,
        false, // skip_recorded
        false, // rerecord
    )
    .await?;

    if final_failed.is_empty() {
        println!("\n ✅ Quick Mode: All failed cases successfully re-recorded and verified.");
    } else {
        println!(
            "\n ❌ Quick Mode: Some cases still failing after re-record: {:?}",
            final_failed
        );
    }

    Ok(final_failed)
}

pub async fn run_bench_swe_cmd(
    api_key: &str,
    lang: &str,
    focus: Option<&str>,
    max_repairs: u8,
    delay: u64,
    record: bool,
    replay: bool,
    rerecord: bool,
) -> Result<()> {
    crate::bench_swe::run_bench_swe(
        api_key,
        lang,
        max_repairs,
        delay,
        focus,
        record,
        replay,
        rerecord,
    )
    .await
}
