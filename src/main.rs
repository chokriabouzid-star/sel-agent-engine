mod context;
// src/main.rs — v1.3
mod types;
mod protocol;
mod executor;
mod llm;
mod agent;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
#[command(name = "sel-agent", version = "1.3.0")]
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
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run { workspace, goal, max_repairs, dry_run } => {
            println!("\n╔══════════════════════════════════════════╗");
            println!("║   SEL Agent v1.3 — State Machine Engine  ║");
            println!("╚══════════════════════════════════════════╝");
            println!("\n📋 Goal: \"{}\"", goal);
            println!("   Workspace:   {}", workspace.display());
            println!("   Max repairs: {}", max_repairs);
            // ── Dry Run v1.3 ──────────────────────────────
            if dry_run {
                println!("   Mode:         🔍 DRY RUN (preview only — nothing will execute)\n");
                let api_key = std::env::var("GROQ_API_KEY")
                    .expect("GROQ_API_KEY not set");
                let llm = llm::LlmClient::new(api_key);
                let prompt = format!("Goal: {}\n\nProvide the complete execution plan.", goal);
                match llm.call(&[types::Message::user(prompt)]).await {
                    Ok(response) => match protocol::parse(&response) {
                        Ok(plan) => {
                            println!("📋 Plan preview ({} commands):\n", plan.commands.len());
                            for (i, cmd) in plan.commands.iter().enumerate() {
                                println!("  [{}/{}] {}", i + 1, plan.commands.len(), cmd.label());
                            }
                            println!("\n✅ DRY RUN complete — nothing was executed.");
                        }
                        Err(e) => println!("❌ Plan parse error: {}", e),
                    },
                    Err(e) => println!("❌ LLM error: {}", e),
                }
                return Ok(());
            }
            // ── Normal Run ────────────────────────────────
            let api_key = std::env::var("GROQ_API_KEY")
                .expect("GROQ_API_KEY not set");
            std::fs::create_dir_all(&workspace)?;
            let mut ag = agent::Agent::new(api_key, workspace, goal, max_repairs);
            ag.run().await?;
        }
    }
    Ok(())
}
