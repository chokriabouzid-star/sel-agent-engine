// src/main.rs — v0.4

mod types;
mod protocol;
mod executor;
mod llm;
mod agent;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sel-agent", version = "0.4.0")]
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
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run { workspace, goal, max_repairs } => {
            let api_key = std::env::var("GROQ_API_KEY")
                .expect("GROQ_API_KEY not set");

            println!("\n╔══════════════════════════════════════════╗");
            println!("║   SEL Agent v0.4 — State Machine Engine  ║");
            println!("╚══════════════════════════════════════════╝");
            println!("\n📋 Goal: \"{}\"", goal);
            println!("   Workspace:   {}", workspace.display());
            println!("   Max repairs: {}", max_repairs);

            std::fs::create_dir_all(&workspace)?;

            let mut ag = agent::Agent::new(api_key, workspace, goal, max_repairs);
            ag.run().await?;
        }
    }
    Ok(())
}
