#![allow(clippy::manual_strip)]
#![allow(clippy::if_same_then_else)]
use anyhow::Result;

use crate::{agent, types};

pub async fn run_plan(
    api_key: &str,
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
        println!("\n No pending tasks found in plan file.");
        println!("   Use format: - [ ] your goal here");
        return Ok(());
    }

    println!("\n");
    println!("   SEL Agent  Markdown Plan Runner        ");
    println!();
    println!(
        "  Plan:       {:<27}",
        plan_file.file_name().unwrap_or_default().to_string_lossy()
    );
    println!("  Tasks:      {:<27}", tasks.len());
    println!(
        "  Workspace:  {:<27}",
        workspace
            .display()
            .to_string()
            .chars()
            .take(27)
            .collect::<String>()
    );
    println!("\n");

    std::fs::create_dir_all(workspace).ok();

    let mut passed = 0usize;
    let mut total_repairs = 0usize;

    for (i, task) in tasks.iter().enumerate() {
        println!("\n Task {}/{} ", i + 1, tasks.len());
        println!("    {}", &task.chars().take(80).collect::<String>());

        let pb = indicatif::ProgressBar::new_spinner();
        pb.set_style(
            indicatif::ProgressStyle::default_spinner()
                .template(&format!(
                    "{{spinner:.cyan}}   Task [{}/{}]...",
                    i + 1,
                    tasks.len()
                ))
                .expect("progress bar template"),
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
            println!("    Passed (repairs: {})", repairs);
        } else {
            println!("    Failed (repairs: {})", repairs);
        }
    }

    let avg_repairs = if !tasks.is_empty() {
        total_repairs as f64 / tasks.len() as f64
    } else {
        0.0
    };

    println!("\n");
    println!("   Plan Results                            ");
    println!();
    println!("  Tasks:      {:<27}", tasks.len());
    println!(
        "  Passed:     {:<27}",
        format!("{}/{}", passed, tasks.len())
    );
    println!("  Avg Repairs:{:<27}", format!("{:.1}", avg_repairs));
    println!("\n");

    Ok(())
}
