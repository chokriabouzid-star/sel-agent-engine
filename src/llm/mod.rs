// src/llm/mod.rs
pub mod json_sanitizer;
pub mod key_pool;
pub mod limit_tracker;
pub mod live;
pub mod record;
pub mod replay;

use crate::types::Message;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub temperature: f32,
    pub seed: Option<u64>,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub finish_reason: String,
    #[serde(default)]
    pub provider_model: Option<String>,
    #[serde(default)]
    pub provider_used: String,
    #[serde(default)]
    pub task_kind: String,
}

pub fn get_system_prompt(bench_mode: bool) -> String {
    let mut prompt = r#"You are SEL Agent, an autonomous execution engine.
CRITICAL: Respond ONLY with a valid JSON object matching the schema below. No markdown text outside the JSON block.

SCHEMA:
{"version":"1.0","commands":[
  {"type":"run","command":"..."},
  {"type":"write_file","path":"...","content":"..."},
  {"type":"run_tests","target":"..."},
  {"type":"patch_file","path":"...","search":"...","replace":"..."},
  {"type":"done","message":"..."}
]}

RULES:
- Python: Use venv/bin/pytest
- Rust: run_tests target MUST be "cargo"
- Rust: cargo new creates dummy src/lib.rs. ALWAYS use write_file to completely overwrite it, NEVER patch it.
- Go: run_tests target MUST be "go", ALWAYS import "fmt"/"errors" if used
- TS/Node: run_tests target MUST be "npm test"
- STRONG TESTS: Write comprehensive tests with both positive and negative cases.

JSON SAFETY  MANDATORY:
1. Use \n for newlines inside content strings, NEVER raw line breaks.
2. Use \" for quotes inside content, NEVER unescaped quotes.
3. NEVER put arrow functions (=>) inside JSON content strings.
4. Keep each content value SHORT (< 200 chars per line).
5. For complex files: split into multiple write_file commands.
6. NEVER use raw template literals (`...`) inside JSON strings.
"#.to_string();

    if bench_mode {
        prompt.push_str(
            r#"
SPEC FILE PROTECTION  MANDATORY:
- NEVER modify existing test files (test_*.py, *_test.go, *.test.ts, *.spec.ts).
- If tests fail, fix the SOURCE code, NOT the tests.
- Creating NEW test files is allowed; modifying EXISTING ones is FORBIDDEN.
"#,
        );
    } else {
        prompt.push_str(
            r#"
SPEC FILE PROTECTION  RELAXED (RUN MODE):
- You may augment existing test files with NEW test cases to cover edge cases.
- NEVER delete or alter the logic of existing test cases.
"#,
        );
    }

    prompt
}

#[derive(Debug, Clone, Default)]
pub struct LlmCallStats {
    pub retries: u32,
    pub connection_errors: u32,
    pub rate_limits: u32,
    pub timeouts: u32,
    pub total_latency_ms: u64,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub successful_calls: u32,
    pub last_model: String,
}

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn complete(&self, req: LLMRequest) -> anyhow::Result<LLMResponse>;
    fn mode(&self) -> &'static str;
    fn get_stats(&self) -> LlmCallStats {
        LlmCallStats::default()
    }
}

pub fn classify_json_error(reason: &str) -> String {
    if reason.contains("No ```json") || reason.contains("json block") {
        "  []    JSON    prompt ".to_string()
    } else if reason.contains("missing field") {
        "  [] JSON   ".to_string()
    } else {
        format!(
            "  []   JSON  {}",
            reason.chars().take(50).collect::<String>()
        )
    }
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub model_id: String,
    pub base_url: String,
    pub env_key: String,
}

impl ModelConfig {
    pub fn from_alias(alias: &str) -> Self {
        match alias {
            "kimi" | "kimi-k2" | "kimi-k2-instruct" => ModelConfig {
                model_id: "moonshotai/kimi-k2-instruct-0905".to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key: "GROQ_API_KEY".to_string(),
            },
            "llama" | "llama-70b" => ModelConfig {
                model_id: "llama-3.3-70b-versatile".to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key: "GROQ_API_KEY".to_string(),
            },
            "kimi-k2.5" | "kimi25" | "kimi-latest" => ModelConfig {
                model_id: "kimi-k2.5".to_string(),
                base_url: "https://api.moonshot.ai/v1/chat/completions".to_string(),
                env_key: "MOONSHOT_API_KEY".to_string(),
            },
            "silicon" | "kimi-silicon" => ModelConfig {
                model_id: "moonshotai/Kimi-K2.5".to_string(),
                base_url: "https://api.siliconflow.cn/v1/chat/completions".to_string(),
                env_key: "SILICONFLOW_API_KEY".to_string(),
            },
            _ => ModelConfig {
                model_id: alias.to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key: "GROQ_API_KEY".to_string(),
            },
        }
    }
}

/// v7.9.9 P2: Preflight Quota Check
/// Estimates if available keys can handle the task count
pub fn preflight_quota_check(task_count: usize, provider: &dyn LLMProvider) {
    let mode = provider.mode();
    if mode == "replay" {
        return;
    }

    // Estimate: 1 planning + 1 repair per task = 2 calls per task
    let total_estimated = task_count * 2;
    println!("\n 📊 [Quota] Preflight check:");
    println!("   Tasks:      {}", task_count);
    println!(
        "   Est. Calls: {} (planning + avg repairs)",
        total_estimated
    );

    // In live mode, we can show configured provider count
    if mode == "live" {
        let cache = crate::provider_state::ProviderStateCache::load();

        // Count keys from env directly to know total available across runs
        let groq_keys = crate::llm::key_pool::KeyPool::from_env("GROQ_API_KEY")
            .keys
            .len();
        let gemini_keys = crate::llm::key_pool::KeyPool::from_env("GEMINI_API_KEY")
            .keys
            .len();
        let cerebras_keys = crate::llm::key_pool::KeyPool::from_env("CEREBRAS_API_KEY")
            .keys
            .len();
        let openrouter_keys = crate::llm::key_pool::KeyPool::from_env("OPENROUTER_API_KEY")
            .keys
            .len();
        let github_keys = crate::llm::key_pool::KeyPool::from_env("GITHUB_TOKEN")
            .keys
            .len();

        let providers = vec![
            ("GROQ_API_KEY", groq_keys),
            ("GEMINI_API_KEY", gemini_keys),
            ("CEREBRAS_API_KEY", cerebras_keys),
            ("OPENROUTER_API_KEY", openrouter_keys),
            ("GITHUB_TOKEN", github_keys),
        ];

        let remaining = cache.estimated_remaining_calls(&providers);
        println!(
            "   Est. Remaining Capacity: ~{} calls across all providers",
            remaining
        );

        if total_estimated > 30 {
            println!(
                "   ⚠️  HIGH LOAD: {} tasks may exhaust free-tier quotas quickly.",
                task_count
            );
        } else {
            println!("    Load seems manageable for the configured providers.");
        }

        if remaining < total_estimated {}
    }
}
