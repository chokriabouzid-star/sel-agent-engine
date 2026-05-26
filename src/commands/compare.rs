use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::{evaluator, types};


pub async fn run_compare(models: &[String], suite: &str, max_repairs: u8) -> Result<()> {
    println!("\n");
    println!("   SEL Agent v7.6.0  Model Comparison       ");
    println!("\n");
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

    let cases = crate::bench_cases::suite_cases(suite);

    if cases.is_empty() {
        println!(
            " Unknown suite '{}'. Use: python, go, node, rust, typescript, all",
            suite
        );
        return Ok(());
    }

    let tmpdir = std::env::temp_dir();
    let mut results: Vec<ModelResult> = Vec::new();

    for model_alias in models {
        println!(
            "\n Testing model: {} ",
            model_alias
        );

        let model_cfg = crate::llm::ModelConfig::from_alias(model_alias);
        let api_key = std::env::var(&model_cfg.env_key).unwrap_or_else(|_| {
            println!(
                "    {}      {}",
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

        for (i, case) in cases.iter().enumerate() {
            let name = &case.name;
            let goal = &case.goal;
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

            let cfg = crate::llm::ModelConfig::from_alias(&model_alias);
            let llm = Box::new(crate::llm::live::LiveProvider::from_config(
                cfg,
                api_key.clone(),
            ));

            let mut agent = crate::agent::Agent::new_with_model(
                api_key.clone(),
                model_alias.clone(),
                workspace.clone(),
                goal.to_string(),
                max_repairs,
                types::ContextConfig::default(),
                llm,
            );
            agent.bench_mode = true; // v7.9.8: skip EXPLAIN MODE
            let run_result = agent.run().await;
            let ok = run_result.is_ok();
            pb.finish_and_clear();

            if let Err(ref e) = run_result {
                println!(
                    "    {:20} FAILED: {}",
                    name,
                    &e.to_string()[..e.to_string().len().min(80)]
                );
                let _ = std::fs::remove_dir_all(&workspace);
                continue;
            }

            let repairs = agent.repair_count();
            total_repairs += repairs;
            // v6.1:   
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
                "".to_string()
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

    //  v6.1: RAS + DTO 
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
            "  : {} (Composite: {:.3} | Correct: {:.2} | Reliable: {:.2})\n",
            best.model, best.composite, best.correctness, best.reliability
        );
    } else {
        println!("        \n");
    }

    Ok(())
}

