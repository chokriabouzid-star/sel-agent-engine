// src/llm/live.rs

use super::{LLMProvider, LLMRequest, LLMResponse, LlmCallStats};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ─── Provider ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Provider {
    name: String,
    model: String,
    endpoint: String,
    api_key: String,
    daily_limit: String,
}

impl Provider {
    pub fn from_id(id: &crate::llm::quota::ProviderId) -> Self {
        match id {
            crate::llm::quota::ProviderId::Cerebras => Provider {
                name: "Cerebras".into(),
                model: std::env::var("CEREBRAS_MODEL").unwrap_or_else(|_| "llama-3.3-70b".into()),
                endpoint: "https://api.cerebras.ai/v1/chat/completions".into(),
                api_key: std::env::var("CEREBRAS_API_KEY").unwrap_or_default(),
                daily_limit: "".into(),
            },
            crate::llm::quota::ProviderId::Mistral => Provider {
                name: "Mistral".into(),
                model: std::env::var("MISTRAL_MODEL").unwrap_or_else(|_| "devstral-small-2507".into()),
                endpoint: "https://api.mistral.ai/v1/chat/completions".into(),
                api_key: std::env::var("MISTRAL_API_KEY").unwrap_or_default(),
                daily_limit: "".into(),
            },
            crate::llm::quota::ProviderId::Groq => Provider {
                name: "Groq".into(),
                model: std::env::var("GROQ_MODEL").unwrap_or_else(|_| "llama-3.3-70b-versatile".into()),
                endpoint: "https://api.groq.com/openai/v1/chat/completions".into(),
                api_key: std::env::var("GROQ_API_KEY").unwrap_or_default(),
                daily_limit: "".into(),
            },
            crate::llm::quota::ProviderId::OpenRouter => Provider {
                name: "OpenRouter".into(),
                model: std::env::var("OPENROUTER_MODEL").unwrap_or_else(|_| "qwen/qwen3-coder:free".into()),
                endpoint: "https://openrouter.ai/api/v1/chat/completions".into(),
                api_key: std::env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| std::env::var("SEL_API_KEY").unwrap_or_default()),
                daily_limit: "".into(),
            },
            crate::llm::quota::ProviderId::SambaNova => Provider {
                name: "SambaNova".into(),
                model: std::env::var("SAMBANOVA_MODEL").unwrap_or_else(|_| "DeepSeek-V3.2".into()),
                endpoint: "https://api.sambanova.ai/v1/chat/completions".into(),
                api_key: std::env::var("SAMBANOVA_API_KEY").unwrap_or_default(),
                daily_limit: "".into(),
            },
            crate::llm::quota::ProviderId::Gemini => Provider {
                name: "Gemini".into(),
                model: std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.0-flash-lite".into()),
                endpoint: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".into(),
                api_key: std::env::var("GEMINI_API_KEY").unwrap_or_default(),
                daily_limit: "".into(),
            },
            crate::llm::quota::ProviderId::GitHub => Provider {
                name: "GitHub".into(),
                model: std::env::var("GITHUB_MODEL").unwrap_or_else(|_| "gpt-4o".into()),
                endpoint: "https://models.inference.ai.azure.com/chat/completions".into(),
                api_key: std::env::var("GITHUB_TOKEN").unwrap_or_default(),
                daily_limit: "".into(),
            },
        }
    }

    fn key_preview(&self) -> String {
        let k = &self.api_key;
        if k.len() > 12 {
            format!("{}...{}", &k[..8], &k[k.len() - 4..])
        } else if k.len() > 4 {
            format!("{}...", &k[..4])
        } else {
            "***".to_string()
        }
    }
}

// ─── LiveProvider ────────────────────────────────────────────────────────────

use super::spo::SmartProviderOrchestra;

pub struct LiveProvider {
    providers: Vec<Provider>,
    stats: Arc<Mutex<LlmCallStats>>,
    daily_exhausted: Arc<Mutex<HashSet<String>>>,
    orchestra: Arc<SmartProviderOrchestra>,
}

// ─── Serde types ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Request {
    model: String,
    messages: Vec<ApiMsg>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ApiMsg {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct Response {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ApiMsg,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

// ─── impl LiveProvider ───────────────────────────────────────────────────────

impl LiveProvider {
    pub fn from_env() -> Self {
        let mut providers = Vec::new();

        if let Ok(key) = std::env::var("CEREBRAS_API_KEY") {
            if !key.trim().is_empty() {
                let model = std::env::var("CEREBRAS_MODEL")
                    .unwrap_or_else(|_| "qwen-3-235b-a22b-instruct-2507".to_string());
                providers.push(Provider {
                    name: "Cerebras".to_string(),
                    model,
                    endpoint: "https://api.cerebras.ai/v1/chat/completions".to_string(),
                    api_key: key.trim().to_string(),
                    daily_limit: "بلا حد يومي معلن".to_string(),
                });
            }
        }

        if let Ok(key) = std::env::var("GITHUB_TOKEN") {
            if !key.trim().is_empty() {
                let model = std::env::var("GITHUB_MODEL")
                    .unwrap_or_else(|_| "gpt-4o".to_string());
                providers.push(Provider {
                    name: "GitHub".to_string(),
                    model,
                    endpoint: "https://models.inference.ai.azure.com/chat/completions".to_string(),
                    api_key: key.trim().to_string(),
                    daily_limit: "150/day".to_string(),
                });
            }
        }

        if let Ok(key) = std::env::var("GEMINI_API_KEY") {
            let key = key.trim().to_string();
            if !key.is_empty() {
                let model = std::env::var("GEMINI_MODEL")
                    .unwrap_or_else(|_| "gemini-2.0-flash".to_string());
                providers.push(Provider {
                    name: "Gemini".to_string(),
                    model,
                    endpoint:
                        "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
                            .to_string(),
                    api_key: key,
                    daily_limit: "1,500 RPD (free tier)".to_string(),
                });
            }
        }

        if let Ok(key) = std::env::var("GROQ_API_KEY") {
            let key = key.trim().to_string();
            if !key.is_empty() {
                let model = std::env::var("GROQ_MODEL")
                    .unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());
                let name = if model.contains("kimi") {
                    "Groq/Kimi"
                } else {
                    "Groq/Llama"
                };
                providers.push(Provider {
                    name: name.to_string(),
                    model,
                    endpoint: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                    api_key: key,
                    daily_limit: "500,000 tokens/day".to_string(), // Simplified limit
                });
            }
        }

        if let (Ok(base), Ok(key)) = (std::env::var("SEL_API_BASE"), std::env::var("SEL_API_KEY")) {
            if !base.is_empty() && !key.is_empty() {
                let model = std::env::var("OPENROUTER_MODEL")
                    .unwrap_or_else(|_| "qwen/qwen3-coder:free".to_string());
                let endpoint = normalize_endpoint(&base);
                let name = detect_name(&endpoint);
                providers.push(Provider {
                    name,
                    model,
                    endpoint,
                    api_key: key,
                    daily_limit: "Custom".to_string(),
                });
            }
        }

        Self {
            providers,
            stats: Arc::new(Mutex::new(LlmCallStats::default())),
            daily_exhausted: Arc::new(Mutex::new(HashSet::new())),
            orchestra: Arc::new(SmartProviderOrchestra::new()),
        }
    }

    pub fn print_info(&self) {
        if self.providers.is_empty() {
            println!("   ❌ لا يوجد API key صالح");
            println!("   💡 جرّب: export GEMINI_API_KEY=... أو GROQ_API_KEY=...");
            return;
        }

        let p = &self.providers[0];
        println!("   🔌 Provider:  {}", p.name);
        println!("   🤖 Model:     {}", p.model);
        println!("   🔑 Key:       {}", p.key_preview());
        println!("   📊 Limit:     {}", p.daily_limit);

        if self.providers.len() > 1 {
            let fallbacks: Vec<&str> = self.providers[1..]
                .iter()
                .map(|p| p.name.as_str())
                .collect();
            println!("   🔄 Fallback:  {}", fallbacks.join(" → "));
        }
    }

    pub fn primary_name(&self) -> String {
        self.providers
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "unknown".to_string())
    }

    pub fn from_config(cfg: crate::llm::ModelConfig, api_key: String) -> Self {
        let name = if cfg.model_id.contains("kimi") {
            "Kimi".to_string()
        } else if cfg.model_id.contains("llama") {
            "Llama".to_string()
        } else {
            "Custom".to_string()
        };

        let p = Provider {
            name,
            model: cfg.model_id,
            endpoint: cfg.base_url,
            api_key,
            daily_limit: "Compare Mode".to_string(),
        };

        Self {
            providers: vec![p],
            stats: Arc::new(Mutex::new(LlmCallStats::default())),
            daily_exhausted: Arc::new(Mutex::new(HashSet::new())),
            orchestra: Arc::new(SmartProviderOrchestra::new()),
        }
    }
}

#[async_trait]
impl LLMProvider for LiveProvider {
    async fn complete(&self, req: LLMRequest) -> Result<LLMResponse> {
        if self.providers.is_empty() {
            return Err(anyhow!("❌ لا يوجد API key"));
        }

        // Detect language from prompt if possible
        let prompt_text = req.messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join(" ");
        let goal_text = if let Some(start) = prompt_text.find("Goal: ") {
            let rest = &prompt_text[start + 6..];
            if let Some(end) = rest.find("\n\nATTEMPT INFO:") {
                &rest[..end]
            } else if let Some(end) = rest.find("\n\n") {
                &rest[..end]
            } else {
                rest
            }
        } else {
            prompt_text.as_str()
        };

        let lang = if goal_text.to_lowercase().contains("python") || goal_text.to_lowercase().contains("pytest") { "python" }
            else if goal_text.to_lowercase().contains("rust") || goal_text.to_lowercase().contains("cargo") { "rust" }
            else if goal_text.to_lowercase().contains("go ") || goal_text.to_lowercase().contains("golang") { "go" }
            else if goal_text.to_lowercase().contains("typescript") || goal_text.to_lowercase().contains("ts") { "typescript" }
            else if goal_text.to_lowercase().contains("node") || goal_text.to_lowercase().contains("javascript") || goal_text.to_lowercase().contains("js") { "javascript" }
            else { "auto" };
            
        let error_context = if prompt_text.contains("FAILED STEPS:") {
            Some(prompt_text.as_str())
        } else {
            None
        };
        
        let repairs_needed = if prompt_text.contains("ATTEMPT ") { 1 } else { 0 };

        match self.orchestra.select_and_call(&req, goal_text, lang, error_context, repairs_needed).await {
            Ok((mut resp, prov, kind)) => {
                resp.provider_used = prov;
                resp.task_kind = kind;
                resp.spo_version = "v2.1".into();
                
                // Update global stats
                let mut lstats = self.stats.lock().unwrap();
                lstats.tokens_in += resp.tokens_in;
                lstats.tokens_out += resp.tokens_out;
                lstats.successful_calls += 1;
                lstats.last_model = resp.provider_used.clone();
                
                Ok(resp)
            }
            Err(e) => Err(anyhow!("SPO Error: {}", e)),
        }
    }

    fn mode(&self) -> &'static str {
        "live"
    }

    fn get_stats(&self) -> LlmCallStats {
        self.stats.lock().unwrap().clone()
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn normalize_endpoint(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/chat/completions") {
        b.to_string()
    } else {
        format!("{}/chat/completions", b)
    }
}

fn detect_name(endpoint: &str) -> String {
    if endpoint.contains("openrouter") {
        "OpenRouter".to_string()
    } else {
        "Custom".to_string()
    }
}

pub async fn call_provider_api(provider_id: &crate::llm::quota::ProviderId, req_meta: &LLMRequest) -> Result<LLMResponse> {
    let p = Provider::from_id(provider_id);
    let msgs = req_meta.messages.clone().into_iter().map(|m| ApiMsg { role: m.role, content: m.content }).collect::<Vec<_>>();
    let stats = Arc::new(Mutex::new(LlmCallStats::default()));
    call_one(&p, &msgs, req_meta, &stats).await
}

async fn call_one(
    provider: &Provider,
    msgs: &[ApiMsg],
    req_meta: &LLMRequest,
    stats: &Arc<Mutex<LlmCallStats>>,
) -> Result<LLMResponse> {
    let start = Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()?;

    // SPO handles retries and failovers, so we remove the local blocking loop.
    let mut body = Request {
        model: provider.model.clone(),
        messages: msgs.to_vec(),
        temperature: if req_meta.temperature > 0.2 { 0.2 } else { req_meta.temperature },
        seed: req_meta.seed,
        max_tokens: 8192,
        response_format: Some(serde_json::json!({ "type": "json_object" })),
    };

    if provider.endpoint.contains("googleapis") || provider.name.to_lowercase().contains("gemini") {
        body.seed = None;
    }

    if provider.name.to_lowercase().contains("sambanova") || provider.name.to_lowercase().contains("cerebras") {
        body.response_format = None;
    }

    let mut req_builder = client.post(&provider.endpoint).json(&body).bearer_auth(&provider.api_key);
    if provider.name.to_lowercase() == "openrouter" {
        req_builder = req_builder.header("HTTP-Referer", "https://github.com/sel-agent").header("X-Title", "SEL Agent");
    }

    let resp = match req_builder.send().await {
        Ok(r) => r,
        Err(e) => {
            stats.lock().unwrap().connection_errors += 1;
            stats.lock().unwrap().total_latency_ms += start.elapsed().as_millis() as u64;
            return Err(anyhow::anyhow!("Connection error: {}", e));
        }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        stats.lock().unwrap().rate_limits += 1;
        let body_err = resp.text().await.unwrap_or_default();
        stats.lock().unwrap().total_latency_ms += start.elapsed().as_millis() as u64;
        return Err(anyhow::anyhow!("API {} — {}", status, &body_err[..body_err.len().min(200)]));
    }

    if !status.is_success() {
        stats.lock().unwrap().connection_errors += 1;
        let body_err = resp.text().await.unwrap_or_default();
        stats.lock().unwrap().total_latency_ms += start.elapsed().as_millis() as u64;
        return Err(anyhow::anyhow!("API {} — {}", status, &body_err[..body_err.len().min(200)]));
    }

        let data: Response = resp.json().await?;
        stats.lock().unwrap().total_latency_ms += start.elapsed().as_millis() as u64;

        let choice = data
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("Empty response"))?;
        let usage = data.usage.unwrap_or(Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
        });

        {
            let mut lstats = stats.lock().unwrap();
            lstats.tokens_in += usage.prompt_tokens;
            lstats.tokens_out += usage.completion_tokens;
            lstats.successful_calls += 1;
            lstats.last_model = format!("{}/{}", provider.name, provider.model);
        }

        return Ok(LLMResponse {
            content: choice.message.content,
            tokens_in: usage.prompt_tokens,
            tokens_out: usage.completion_tokens,
            finish_reason: choice.finish_reason.unwrap_or_else(|| "stop".into()),
            provider_model: Some(format!("{}/{}", provider.name, provider.model)),
            provider_used: String::new(),
            task_kind: String::new(),
            spo_version: String::new(),
        });
}
