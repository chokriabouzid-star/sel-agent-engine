// src/main.rs — SEL Agent v1.5
mod context;
mod types;
mod protocol;
mod executor;
mod llm;
mod agent;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "sel-agent", version = "1.5.0")]
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
    Health,
    Stress {
        #[arg(long, default_value = "3")] max_repairs: u8,
    },
}

async fn run_health(api_key: &str) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent v1.5 — Health Check          ║");
    println!("╚══════════════════════════════════════════╝\n");

    let internet = reqwest::Client::new()
        .get("https://1.1.1.1")
        .timeout(Duration::from_secs(5))
        .send().await;
    match internet {
        Ok(_)  => println!("🌐 Internet:     {}", "✅ Connected".green()),
        Err(_) => println!("🌐 Internet:     {}", "❌ No connection".red()),
    }

    let groq = reqwest::Client::new()
        .get("https://api.groq.com")
        .timeout(Duration::from_secs(5))
        .send().await;
    match groq {
        Ok(_)  => println!("🔌 Groq Server:  {}", "✅ Reachable".green()),
        Err(_) => println!("🔌 Groq Server:  {}", "❌ Unreachable".red()),
    }

    let key_preview = if api_key.len() > 8 {
        format!("{}...", &api_key[..8])
    } else { "???".to_string() };
    println!("🔑 API Key:      {} ({})", "✅ Set".green(), key_preview);

    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner()
        .template("{spinner:.cyan} 🤖 Model:        Testing response...").unwrap());
    pb.enable_steady_tick(Duration::from_millis(100));

    let llm = llm::LlmClient::new(api_key.to_string());
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
    println!("⚙️  SEL Binary:   {} ({})", "✅ Built".green(), binary.display());
    println!();
    Ok(())
}

async fn run_stress(api_key: &str, max_repairs: u8) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent v1.5 — Stress Test           ║");
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
        ("runtime error",    "Create Python function divide(a,b) returning None if b==0 else a/b. Write pytest test for divide(10,0)==None. Run tests."),
    ];

    let total = cases.len();
    let mut passed = 0usize;
    let mut total_repairs = 0usize;
    let tmpdir = std::env::temp_dir();

    for (i, (name, goal)) in cases.iter().enumerate() {
        let workspace = tmpdir.join(format!("sel-stress-{}", i));
        let _ = std::fs::remove_dir_all(&workspace);

        let pb = ProgressBar::new_spinner();
        pb.set_style(ProgressStyle::default_spinner()
            .template(&format!("{{spinner:.yellow}} ⏳ Running: {}...", name)).unwrap());
        pb.enable_steady_tick(Duration::from_millis(80));

        let mut ag = agent::Agent::new(
            api_key.to_string(),
            workspace.clone(),
            goal.to_string(),
            max_repairs,
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

    let avg = if passed > 0 { total_repairs as f64 / passed as f64 } else { 0.0 };
    println!("\n=== Stress Results: {}/{} passed | avg repairs: {:.1} ===\n", passed, total, avg);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Health => {
            let api_key = std::env::var("GROQ_API_KEY").expect("GROQ_API_KEY not set");
            run_health(&api_key).await?;
        }
        Commands::Stress { max_repairs } => {
            let api_key = std::env::var("GROQ_API_KEY").expect("GROQ_API_KEY not set");
            run_stress(&api_key, max_repairs).await?;
        }
        Commands::Run { workspace, goal, max_repairs, dry_run } => {
            println!("\n╔══════════════════════════════════════════╗");
            println!("║   SEL Agent v1.5 — State Machine Engine  ║");
            println!("╚══════════════════════════════════════════╝");
            println!("\n📋 Goal: \"{}\"", goal);
            println!("   Workspace:   {}", workspace.display());
            println!("   Max repairs: {}", max_repairs);

            let api_key = std::env::var("GROQ_API_KEY").expect("GROQ_API_KEY not set");

            if dry_run {
                println!("   Mode:         🔍 DRY RUN\n");
                let llm = llm::LlmClient::new(api_key);
                let prompt = format!("Goal: {}\n\nProvide the complete execution plan.", goal);
                match llm.call(&[types::Message::user(prompt)]).await {
                    Ok(response) => match protocol::parse(&response) {
                        Ok(plan) => {
                            println!("📋 Plan preview ({} commands):\n", plan.commands.len());
                            for (i, cmd) in plan.commands.iter().enumerate() {
                                println!("  [{}/{}] {}", i+1, plan.commands.len(), cmd.label());
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
            let mut ag = agent::Agent::new(api_key, workspace, goal, max_repairs);
            ag.run().await?;
        }
    }
    Ok(())
}
