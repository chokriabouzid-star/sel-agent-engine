use anyhow::Result;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

use crate::llm::LLMProvider;
use crate::{llm, types};

pub async fn run_health(api_key: &str) -> Result<()> {
    println!("\n");
    println!(
        "   SEL Agent v{}  Health Check                   ",
        env!("CARGO_PKG_VERSION")
    );
    println!("\n");
    // Provider info   bench
    {
        let mdl = std::env::var("SEL_MODEL").unwrap_or_else(|_| "kimi".to_string());
        let (_ep, _key) = if let Ok(base) = std::env::var("SEL_API_BASE") {
            let k = std::env::var("SEL_API_KEY").unwrap_or_default();
            (base, k)
        } else if mdl.contains("gemini") || mdl.starts_with("models/") {
            let k = std::env::var("GEMINI_API_KEY").unwrap_or_default();
            (
                "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
                k,
            )
        } else {
            let k = std::env::var("GROQ_API_KEY").unwrap_or_default();
            ("https://api.groq.com/openai/v1".to_string(), k)
        };
        // print_provider_info removed(&ep, &mdl, &key);
        println!();
    }

    let internet = reqwest::Client::new()
        .get("https://1.1.1.1")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    match internet {
        Ok(_) => println!(" Internet:     {}", " Connected".green()),
        Err(_) => println!(" Internet:     {}", " No connection".red()),
    }

    let groq = reqwest::Client::new()
        .get("https://api.groq.com")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    match groq {
        Ok(_) => println!(" Groq Server:  {}", " Reachable".green()),
        Err(_) => println!(" Groq Server:  {}", " Unreachable".red()),
    }

    let api_key_str = if api_key.is_empty() {
        std::env::var("OPENROUTER_API_KEY")
            .or_else(|_| std::env::var("GEMINI_API_KEY"))
            .or_else(|_| std::env::var("GROQ_API_KEY"))
            .unwrap_or_default()
    } else {
        api_key.to_string()
    };
    let api_key = api_key_str.as_str();
    let key_preview = if api_key.chars().count() > 8 {
        format!("{}...", api_key.chars().take(8).collect::<String>())
    } else {
        "???".to_string()
    };
    println!(" API Key:      {} ({})", " Set".green(), key_preview);

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan}  Model:        Testing response...")
            .expect("progress bar template"),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    let llm = llm::live::LiveProvider::from_env();
    let test_msg = types::Message::user("Reply with exactly: PONG".to_string());
    let req = llm::LLMRequest {
        system: llm::get_system_prompt(false),
        messages: vec![test_msg],
        temperature: 0.0,
        seed: Some(42),
        model: "default".into(),
    };
    match llm.complete(req).await {
        Ok(resp) => {
            pb.finish_and_clear();
            if !resp.content.is_empty() {
                println!(" Model:        {}", " Responding".green());
            } else {
                println!(" Model:        {}", "  Empty response".yellow());
            }
        }
        Err(e) => {
            pb.finish_and_clear();
            println!(" Model:        {}  {}", " Failed".red(), e);
        }
    }

    let binary = std::env::current_exe().unwrap_or_default();
    println!(
        "  SEL Binary:   {} ({})",
        " Built".green(),
        binary.display()
    );
    println!();
    Ok(())
}

#[derive(Default)]
pub struct ProviderStats {
    pub call_counts: std::collections::HashMap<String, u32>,
}

impl ProviderStats {
    pub fn record(&mut self, provider_name: &str) {
        *self
            .call_counts
            .entry(provider_name.to_string())
            .or_insert(0) += 1;
    }

    pub fn total(&self) -> u32 {
        self.call_counts.values().sum()
    }
}

pub fn shorten_provider(name: &str) -> &str {
    if name.contains("groq") || name.contains("llama-3.3") {
        "groq"
    } else if name.contains("gpt-4o") || name.contains("github") {
        "github/gpt-4o"
    } else if name.contains("gemini") {
        "gemini"
    } else if name.contains("cerebras") || name.contains("qwen-3") {
        "cerebras"
    } else if name.contains("openrouter") {
        "openrouter"
    } else {
        name
    }
}
