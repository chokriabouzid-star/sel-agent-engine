use crate::report::ExecutionReport;
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

fn reports_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".sel-agent")
        .join("reports")
}

fn load_reports(max: usize) -> Vec<ExecutionReport> {
    let dir = reports_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let mut files: Vec<_> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let n = name.to_string_lossy();
            n.ends_with(".json") && n != "latest.json"
        })
        .collect();

    files.sort_by_key(|b| std::cmp::Reverse(b.file_name()));
    files.truncate(max);

    files
        .into_iter()
        .filter_map(|e| {
            let data = fs::read_to_string(e.path()).ok()?;
            serde_json::from_str::<ExecutionReport>(&data).ok()
        })
        .collect()
}

pub fn run_report(latest: bool, summary: bool, count: usize) -> Result<()> {
    if latest {
        print_latest()?;
    } else if summary {
        print_summary(count)?;
    } else {
        print_latest()?;
    }
    Ok(())
}

fn print_latest() -> Result<()> {
    let path = reports_dir().join("latest.json");
    if !path.exists() {
        println!("No reports found. Run a task first.");
        return Ok(());
    }

    let data = fs::read_to_string(&path)?;
    let r: ExecutionReport = serde_json::from_str(&data)?;

    let icon = match r.outcome {
        crate::report::ExecutionOutcome::Pass => "✅",
        crate::report::ExecutionOutcome::Fail => "❌",
    };

    println!();
    println!("╔══════════════════════════════════════════════════╗");
    println!("║          SEL Agent — Latest Report               ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  {} Outcome:  {:42}║", icon, format!("{:?}", r.outcome));
    println!("║  Goal:     {:42}║", truncate(&r.goal, 42));
    println!(
        "║  Hash:     {:42}║",
        &r.goal_hash[..r.goal_hash.len().min(16)]
    );
    println!("║  Mode:     {:42}║", r.mode);
    println!("║  Duration: {:42}║", format!("{}s", r.duration_secs));
    println!("║  Repairs:  {:42}║", r.repair_attempts);
    println!("║  AutoFix:  {:42}║", r.autofix_count);
    println!("║  Tests OK: {:42}║", r.tests_passed);
    println!("║  Mutation: {:42}║", format_mutation(r.mutation_score));
    println!("║  Model:    {:42}║", truncate(&r.provider_model, 42));
    println!(
        "║  LLM:      {:42}║",
        format!(
            "{} calls, {}+{} tokens",
            r.llm_calls, r.tokens_in, r.tokens_out
        )
    );
    println!("║  Time:     {:42}║", r.timestamp_utc);
    if let Some(ref reason) = r.failure_reason {
        println!("║  Reason:   {:42}║", truncate(reason, 42));
    }
    println!("╚══════════════════════════════════════════════════╝");
    println!("  File: {}", path.display());
    println!();

    Ok(())
}

fn print_summary(count: usize) -> Result<()> {
    let reports = load_reports(count);
    if reports.is_empty() {
        println!("No reports found. Run some tasks first.");
        return Ok(());
    }

    let total = reports.len();
    let passed = reports
        .iter()
        .filter(|r| matches!(r.outcome, crate::report::ExecutionOutcome::Pass))
        .count();
    let failed = total - passed;
    let avg_repairs: f64 = if total > 0 {
        reports
            .iter()
            .map(|r| r.repair_attempts as f64)
            .sum::<f64>()
            / total as f64
    } else {
        0.0
    };
    let avg_duration: f64 = if total > 0 {
        reports.iter().map(|r| r.duration_secs as f64).sum::<f64>() / total as f64
    } else {
        0.0
    };

    let replay_count = reports.iter().filter(|r| r.mode == "replay").count();
    let record_count = reports.iter().filter(|r| r.mode == "record").count();
    let live_count = total - replay_count - record_count;

    println!();
    println!("╔══════════════════════════════════════════════════╗");
    println!("║        SEL Agent — Report Summary                ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  Total runs:     {:32}║", total);
    println!("║  ✅ Passed:       {:32}║", passed);
    println!("║  ❌ Failed:       {:32}║", failed);
    println!(
        "║  Success rate:   {:32}║",
        format!(
            "{:.1}%",
            if total > 0 {
                passed as f64 / total as f64 * 100.0
            } else {
                0.0
            }
        )
    );
    println!("║  Avg repairs:    {:32}║", format!("{:.2}", avg_repairs));
    println!("║  Avg duration:   {:32}║", format!("{:.1}s", avg_duration));
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  By mode:                                       ║");
    println!("║    replay:  {:37}║", replay_count);
    println!("║    record:  {:37}║", record_count);
    println!("║    live:    {:37}║", live_count);
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  Recent runs:                                   ║");

    for r in reports.iter().take(10) {
        let icon = match r.outcome {
            crate::report::ExecutionOutcome::Pass => "✅",
            crate::report::ExecutionOutcome::Fail => "❌",
        };
        println!(
            "║  {} {:6} {:3}s r:{} {:20}║",
            icon,
            r.mode,
            r.duration_secs,
            r.repair_attempts,
            truncate(&r.goal, 20)
        );
    }

    println!("╚══════════════════════════════════════════════════╝");
    println!("  Reports dir: {}", reports_dir().display());
    println!();

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    let out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        format!("{}…", &out[..out.len().saturating_sub(1)])
    } else {
        out
    }
}

fn format_mutation(score: f64) -> String {
    if score < 0.0 {
        "N/A".to_string()
    } else {
        format!("{:.0}%", score * 100.0)
    }
}
