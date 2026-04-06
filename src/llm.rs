use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use crate::types::Message;

const SYSTEM_PROMPT: &str = include_str!("system_prompt.txt");

#[derive(Debug, Clone, Default)]
pub struct LlmCallStats {
    pub retries: u32,
    pub connection_errors: u32,
    pub rate_limits: u32,
    pub timeouts: u32,
    pub total_latency_ms: u64,
}



// ─── v7.0: Multi-Provider Support ─────────────────────────────────────────────
/// Priority: SEL_API_KEY → OPENROUTER_API_KEY → GROQ_API_KEY
pub fn resolve_api_key() -> String {
    // NVIDIA NIM
    if let Ok(k) = std::env::var("NVIDIA_API_KEY") {
        if !k.is_empty() { return k; }
    }
    if let Ok(k) = std::env::var("SEL_API_KEY") {
        if !k.trim().is_empty() { eprintln!("🔑 Using SEL_API_KEY"); return k; }
    }
    let base = std::env::var("SEL_API_BASE")
        .or_else(|_| std::env::var("OPENAI_BASE_URL"))
        .unwrap_or_default();
    if base.contains("groq.com") {
        if let Ok(k) = std::env::var("GROQ_API_KEY") {
            if !k.trim().is_empty() { eprintln!("🔑 Using GROQ_API_KEY"); return k; }
        }
    }
    if base.contains("openrouter.ai") {
        if let Ok(k) = std::env::var("OPENROUTER_API_KEY") {
            if !k.trim().is_empty() { eprintln!("🔑 Using OPENROUTER_API_KEY"); return k; }
        }
    }
    if let Ok(k) = std::env::var("GROQ_API_KEY") {
        if !k.trim().is_empty() { eprintln!("🔑 Using GROQ_API_KEY"); return k; }
    }
    if let Ok(k) = std::env::var("OPENROUTER_API_KEY") {
        if !k.trim().is_empty() { eprintln!("🔑 Using OPENROUTER_API_KEY"); return k; }
    }
    panic!("❌ No API key found. Set GROQ_API_KEY or OPENROUTER_API_KEY");
}

/// Priority: SEL_MODEL → default kimi-k2
pub fn resolve_model() -> String {
    if let Ok(m) = std::env::var("SEL_MODEL") {
        if !m.trim().is_empty() { return m; }
    }
    "moonshotai/kimi-k2-instruct".to_string()
}

/// Priority: SEL_API_BASE → OPENAI_BASE_URL → infer from model → Groq
pub fn resolve_base_url(model: &str) -> String {
    if let Ok(base) = std::env::var("SEL_API_BASE") {
        if !base.trim().is_empty() {
            let base = base.trim_end_matches('/');
            return format!("{}/chat/completions", base);
        }
    }
    if let Ok(base) = std::env::var("OPENAI_BASE_URL") {
        if !base.trim().is_empty() {
            let base = base.trim_end_matches('/');
            return format!("{}/chat/completions", base);
        }
    }
    if model.contains("openrouter") {
        return "https://openrouter.ai/api/v1/chat/completions".to_string();
    }
    if model.contains("nvidia") || model.contains("nim") || model.starts_with("meta/") || model.starts_with("mistralai/") {
        return "https://integrate.api.nvidia.com/v1/chat/completions".to_string();
    }
    "https://api.groq.com/openai/v1/chat/completions".to_string()
}

fn resolve_api_key_for(env_key: &str) -> String {
    if let Ok(k) = std::env::var(env_key) {
        if !k.trim().is_empty() { return k; }
    }
    resolve_api_key()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    Groq,
    OpenRouter,
    Moonshot,
    SiliconFlow,
    Custom,
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub provider: Provider,
    pub model_id: String,
    pub base_url: String,
    pub env_key:  String,
}

impl ModelConfig {
    pub fn from_env() -> Self {
        let model    = resolve_model();
        let url      = resolve_base_url(&model);
        let model_id = if model.starts_with("openrouter/") {
            model["openrouter/".len()..].to_string()
        } else { model.clone() };
        let provider = if url.contains("openrouter.ai")  { Provider::OpenRouter }
                       else if url.contains("groq.com")  { Provider::Groq }
                       else if url.contains("moonshot.ai") { Provider::Moonshot }
                       else if url.contains("siliconflow") { Provider::SiliconFlow }
                       else { Provider::Custom };
        ModelConfig { provider, model_id, base_url: url, env_key: String::new() }
    }

    pub fn from_alias(alias: &str) -> Self {
        match alias {
            "kimi" | "kimi-k2" | "kimi-k2-instruct" => ModelConfig {
                provider: Provider::Groq,
                model_id: "moonshotai/kimi-k2-instruct-0905".to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key:  "GROQ_API_KEY".to_string(),
            },
            "llama" | "llama-70b" => ModelConfig {
                provider: Provider::Groq,
                model_id: "llama-3.3-70b-versatile".to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key:  "GROQ_API_KEY".to_string(),
            },
            "nvidia" | "nim" => ModelConfig {
                provider: Provider::Custom,
                model_id: "meta/llama-3.3-70b-instruct".to_string(),
                base_url: "https://integrate.api.nvidia.com/v1/chat/completions".to_string(),
                env_key:  "NVIDIA_API_KEY".to_string(),
            },
            "openrouter" | "or" => ModelConfig {
                provider: Provider::OpenRouter,
                model_id: resolve_model(),
                base_url: "https://openrouter.ai/api/v1/chat/completions".to_string(),
                env_key:  "OPENROUTER_API_KEY".to_string(),
            },
            "kimi25" | "kimi-latest" => ModelConfig {
                provider: Provider::Moonshot,
                model_id: "kimi-k2.5".to_string(),
                base_url: "https://api.moonshot.ai/v1/chat/completions".to_string(),
                env_key:  "MOONSHOT_API_KEY".to_string(),
            },
            "silicon" | "kimi-silicon" => ModelConfig {
                provider: Provider::SiliconFlow,
                model_id: "moonshotai/Kimi-K2.5".to_string(),
                base_url: "https://api.siliconflow.cn/v1/chat/completions".to_string(),
                env_key:  "SILICONFLOW_API_KEY".to_string(),
            },
            // ─── OpenRouter Free Models (v6.9.1) ───────────────────────
            "trinity" | "arcee" | "arcee-trinity" => ModelConfig {
                provider: Provider::OpenRouter,
                model_id: "arcee-ai/trinity-large-preview:free".to_string(),
                base_url: "https://openrouter.ai/api/v1/chat/completions".to_string(),
                env_key:  "OPENROUTER_API_KEY".to_string(),
            },
            "qwen-80b" | "qwen3-80b" | "qwen-next" => ModelConfig {
                provider: Provider::OpenRouter,
                model_id: "qwen/qwen3-next-80b-a3b-instruct".to_string(),
                base_url: "https://openrouter.ai/api/v1/chat/completions".to_string(),
                env_key:  "OPENROUTER_API_KEY".to_string(),
            },
            "qwen-coder" | "qwen3-coder" => ModelConfig {
                provider: Provider::OpenRouter,
                model_id: "qwen/qwen3-coder".to_string(),
                base_url: "https://openrouter.ai/api/v1/chat/completions".to_string(),
                env_key:  "OPENROUTER_API_KEY".to_string(),
            },
            "llama-70b-or" | "llama-or" => ModelConfig {
                provider: Provider::OpenRouter,
                model_id: "meta-llama/llama-3.3-70b-instruct".to_string(),
                base_url: "https://openrouter.ai/api/v1/chat/completions".to_string(),
                env_key:  "OPENROUTER_API_KEY".to_string(),
            },
            "gpt-oss-120b" | "gpt-oss" => ModelConfig {
                provider: Provider::OpenRouter,
                model_id: "openai/gpt-oss-120b".to_string(),
                base_url: "https://openrouter.ai/api/v1/chat/completions".to_string(),
                env_key:  "OPENROUTER_API_KEY".to_string(),
            },
            "hermes" | "hermes-405b" => ModelConfig {
                provider: Provider::OpenRouter,
                model_id: "nousresearch/hermes-3-llama-3.1-405b".to_string(),
                base_url: "https://openrouter.ai/api/v1/chat/completions".to_string(),
                env_key:  "OPENROUTER_API_KEY".to_string(),
            },
            "deepseek" | "deepseek-v3" => ModelConfig {
                provider: Provider::OpenRouter,
                model_id: "deepseek/deepseek-chat-v3-0324".to_string(),
                base_url: "https://openrouter.ai/api/v1/chat/completions".to_string(),
                env_key:  "OPENROUTER_API_KEY".to_string(),
            },
            _ => ModelConfig {
                provider: Provider::Groq,
                model_id: alias.to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key:  "GROQ_API_KEY".to_string(),
            },
        }
    }
}

pub struct LlmClient {
    pub api_key:  String,
    pub model:    String,
    pub endpoint: String,
    pub config:   ModelConfig,
}

#[derive(Serialize)]
struct Request { model: String, messages: Vec<ApiMsg>, temperature: f32, max_tokens: u32 }

#[derive(Serialize, Deserialize, Clone)]
struct ApiMsg { role: String, content: String }

#[derive(Deserialize)]
struct Response { choices: Vec<Choice> }

#[derive(Deserialize)]
struct Choice { message: ApiMsg }

impl LlmClient {
    pub fn from_env() -> Self {
        let config  = ModelConfig::from_env();
        let api_key = resolve_api_key();
        Self { model: config.model_id.clone(), endpoint: config.base_url.clone(), api_key, config }
    }

    pub fn new(api_key: String) -> Self {
        let key     = if api_key.is_empty() { resolve_api_key() } else { api_key };
        let model   = resolve_model();
        let url     = resolve_base_url(&model);
        let model_id = if model.starts_with("openrouter/") {
            model["openrouter/".len()..].to_string()
        } else { model.clone() };
        let provider = if url.contains("openrouter.ai") { Provider::OpenRouter }
                       else if url.contains("groq.com") { Provider::Groq }
                       else { Provider::Custom };
        let config = ModelConfig { provider, model_id: model_id.clone(), base_url: url.clone(), env_key: String::new() };
        Self { api_key: key, model: model_id, endpoint: url, config }
    }

    pub fn with_model(alias: &str) -> Self {
        let config  = ModelConfig::from_alias(alias);
        let api_key = resolve_api_key_for(&config.env_key);
        Self { model: config.model_id.clone(), endpoint: config.base_url.clone(), api_key, config }
    }

    pub async fn call(&self, messages: &[Message]) -> Result<(String, LlmCallStats)> {
        let mut stats      = LlmCallStats::default();
        let call_start     = std::time::Instant::now();
        let system_content = SYSTEM_PROMPT.to_string();
        let mut msgs = vec![ApiMsg { role: "system".into(), content: system_content }];
        for m in messages { msgs.push(ApiMsg { role: m.role.clone(), content: m.content.clone() }); }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()?;
        let delays = [15u64, 45, 120];

        for (attempt, &delay) in delays.iter().enumerate() {
            if attempt > 0 {
                stats.retries += 1;
                println!("   ⏳ Rate limit — retry in {}s...", delay);
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }

            let resp = match client
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(&Request {
                    model:       self.model.clone(),
                    messages:    msgs.clone(),
                    temperature: 0.1,
                    max_tokens:  4096,
                })
                .send().await {
                    Ok(r)  => r,
                    Err(e) => {
                        stats.connection_errors += 1;
                        if attempt + 1 == delays.len() {
                            stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
                            return Err(anyhow!("Connection error: {}", e));
                        }
                        println!("   ⚠ Connection error: {} — retry in {}s...", e, delay);
                        continue;
                    }
                };

            let status = resp.status();

            if status.as_u16() == 413 {
                let body = resp.text().await.unwrap_or_default();
                stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
                return Err(anyhow!("API 413 Payload Too Large — {}", &body[..body.len().min(300)]));
            }

            if status == 429 || status == 503 || status == 502 || status == 500 {
                if status == 429 { stats.rate_limits += 1; } else { stats.connection_errors += 1; }

                // v7.2: كشف daily limit — لا فائدة من retry
                if status == 429 {
                    let body = resp.text().await.unwrap_or_default();
                    if body.contains("per day") || body.contains("TPD") || body.contains("daily") {
                        stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
                        println!("   🔴 Daily token limit reached for this model.");
                        println!("   💡 Solutions:");
                        println!("      1. export SEL_MODEL=llama-3.3-70b-versatile");
                        println!("      2. export SEL_API_BASE=https://openrouter.ai/api/v1");
                        println!("      3. Wait until tomorrow for limit reset");
                        return Err(anyhow!("API 429 Daily Limit — {}", &body[..body.len().min(300)]));
                    }
                    // per-minute limit — retry normally
                    if attempt + 1 == delays.len() {
                        stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
                        return Err(anyhow!("API 429 Too Many Requests — {}", &body[..body.len().min(200)]));
                    }
                    println!("   ⚠ Rate limit (429 Too Many Requests) — retry in {}s...", delay);
                    continue;
                }

                // 503/502/500 — server errors, retry
                if attempt + 1 == delays.len() {
                    let body = resp.text().await.unwrap_or_default();
                    stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
                    return Err(anyhow!("API {} — {}", status, &body[..body.len().min(200)]));
                }
                println!("   ⚠ Server error ({}) — retry in {}s...", status, delay);
                continue;
            }
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow!("API {} — {}", status, &body[..body.len().min(200)]));
            }

            let data: Response = resp.json().await?;
            stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
            let result = data.choices.into_iter().next()
                .map(|c| c.message.content)
                .ok_or_else(|| anyhow!("Empty response"));
            return result.map(|text| (text, stats));
        }
        stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
        Err(anyhow!("LLM failed after all retries"))
    }
}
