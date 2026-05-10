// src/llm/live.rs — LiveProvider بدون SPO
use super::{LLMProvider, LLMRequest, LLMResponse, LlmCallStats};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Provider {
    name: String,
    model: String,
    endpoint: String,
    key_pool: Arc<Mutex<super::key_pool::KeyPool>>,
}

impl Provider {
    fn cerebras() -> Self {
        Provider {
            name: "Cerebras".into(),
            model: std::env::var("CEREBRAS_MODEL").unwrap_or_else(|_| "qwen-3-235b-a22b-instruct-2507".into()),
            endpoint: "https://api.cerebras.ai/v1/chat/completions".into(),
            key_pool: Arc::new(Mutex::new(super::key_pool::KeyPool::from_env("CEREBRAS_API_KEY"))),
        }
    }

    fn github() -> Self {
        Provider {
            name: "GitHub".into(),
            model: std::env::var("GITHUB_MODEL").unwrap_or_else(|_| "gpt-4o".into()),
            endpoint: "https://models.inference.ai.azure.com/chat/completions".into(),
            key_pool: Arc::new(Mutex::new(super::key_pool::KeyPool::from_env("GITHUB_TOKEN"))),
        }
    }

    fn gemini() -> Self {
        Provider {
            name: "Gemini".into(),
            model: std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.0-flash".into()),
            endpoint: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".into(),
            key_pool: Arc::new(Mutex::new(super::key_pool::KeyPool::from_env("GEMINI_API_KEY"))),
        }
    }

    fn groq() -> Self {
        Provider {
            name: "Groq".into(),
            model: std::env::var("GROQ_MODEL").unwrap_or_else(|_| "llama-3.3-70b-versatile".into()),
            endpoint: "https://api.groq.com/openai/v1/chat/completions".into(),
            key_pool: Arc::new(Mutex::new(super::key_pool::KeyPool::from_env("GROQ_API_KEY"))),
        }
    }

    fn openrouter() -> Self {
        let mut pool = super::key_pool::KeyPool::from_env("OPENROUTER_API_KEY");
        if !pool.has_available() {
            pool = super::key_pool::KeyPool::from_env("SEL_API_KEY");
        }
        Provider {
            name: "OpenRouter".into(),
            model: std::env::var("OPENROUTER_MODEL").unwrap_or_else(|_| "qwen/qwen3-coder:free".into()),
            endpoint: "https://openrouter.ai/api/v1/chat/completions".into(),
            key_pool: Arc::new(Mutex::new(pool)),
        }
    }

    fn is_configured(&self) -> bool {
        self.key_pool.lock().unwrap().has_available()
    }

    fn key_preview(&self) -> String {
        let mut pool = self.key_pool.lock().unwrap();
        let key_opt = pool.next_available();
        if let Some(k) = key_opt {
            if k.len() > 12 {
                format!("{}...{}", &k[..8], &k[k.len() - 4..])
            } else if k.len() > 4 {
                format!("{}...", &k[..4])
            } else {
                "***".to_string()
            }
        } else {
            "none".to_string()
        }
    }
}

pub struct LiveProvider {
    providers: Vec<Provider>,
    stats: Arc<Mutex<LlmCallStats>>,
    active_index: std::sync::atomic::AtomicUsize,
    tracker: Arc<Mutex<super::limit_tracker::LimitTracker>>,
}

impl LiveProvider {
    pub fn from_env() -> Self {
        Self::new()
    }

    pub fn from_config(_cfg: crate::llm::ModelConfig, _key: String) -> Self {
        Self::new()
    }

    pub fn primary_name(&self) -> String {
        self.providers.first().map(|p| p.name.clone()).unwrap_or_default()
    }
    pub fn new() -> Self {
        let mut providers = Vec::new();

        // الترتيب: Groq → Gemini → Cerebras → OpenRouter → GitHub
        let candidates = vec![
            Provider::groq(),
            Provider::gemini(),
            Provider::cerebras(),
            Provider::openrouter(),
            Provider::github(),
        ];

        for p in candidates {
            if p.is_configured() {
                providers.push(p);
            }
        }

        if providers.is_empty() {
            panic!("❌ No LLM provider configured. Set at least one API key.");
        }

        LiveProvider {
            providers,
            stats: Arc::new(Mutex::new(LlmCallStats::default())),
            active_index: std::sync::atomic::AtomicUsize::new(0),
            tracker: Arc::new(Mutex::new(super::limit_tracker::LimitTracker::new())),
        }
    }
}

#[derive(Serialize)]
struct RequestBody {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct Response {
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
    finish_reason: String,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

enum ErrorKind {
    DailyLimit,
    RpmLimit,
    Other,
}

fn classify_error(err: &str) -> ErrorKind {
    let lower = err.to_lowercase();
    if lower.contains("per 86400s exceeded")
        || lower.contains("generaterequestsperdayperproject")
        || lower.contains("free_tier_requests")
        || lower.contains("quota")
        || lower.contains("daily limit")
        || lower.contains("per day")
        || lower.contains("401") 
        || lower.contains("unauthorized")
        || lower.contains("404") 
        || lower.contains("not_found")
    {
        return ErrorKind::DailyLimit;
    }
    if lower.contains("per minute")
        || lower.contains("generaterequestsperminuteperproject")
        || lower.contains("generatecontentinputtokenspermodelperminute")
        || lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
    {
        return ErrorKind::RpmLimit;
    }
    ErrorKind::Other
}

#[async_trait::async_trait]
impl LLMProvider for LiveProvider {
    async fn complete(&self, req: LLMRequest) -> Result<LLMResponse> {
        let mut last_error = None;
        let provider_names: Vec<String> = self.providers.iter().map(|p| p.name.clone()).collect();

        {
            let tracker = self.tracker.lock().unwrap();
            if !tracker.any_available(&provider_names) {
                return Err(anyhow!("❌ All providers exhausted for today"));
            }
        }

        for _ in 0..self.providers.len() {
            let idx = self.active_index.load(std::sync::atomic::Ordering::SeqCst);
            let provider = &self.providers[idx];
            let mut attempt = 1;
            let mut rpm_waits = 0;

            loop {
                // Tracker check
                {
                    let tracker = self.tracker.lock().unwrap();
                    if !tracker.is_available(&provider.name) {
                        self.active_index.store((idx + 1) % self.providers.len(), std::sync::atomic::Ordering::SeqCst);
                        break;
                    }
                }
                
                let is_primary = idx == 0;
                let prefix = if is_primary { "🟢" } else { "🔄" };
                if attempt == 1 {
                    println!("{} Calling {} ({})", prefix, provider.name, provider.model);
                } else {
                    println!("   ⚠️  Attempt {}/3 - retrying {}...", attempt, provider.name);
                }

                match self.try_call(provider, &req).await {
                    Ok(mut resp) => {
                        resp.provider_used = provider.name.clone();
                        if let Ok(mut stats) = self.stats.lock() {
                            stats.successful_calls += 1;
                            stats.tokens_in += resp.tokens_in;
                            stats.tokens_out += resp.tokens_out;
                            stats.last_model = provider.model.clone();
                        }
                        return Ok(resp);
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        let err_kind = classify_error(&err_msg);
                        
                        match err_kind {
                            ErrorKind::DailyLimit => {
                                let mut pool = provider.key_pool.lock().unwrap();
                                pool.mark_exhausted();
                                if pool.has_available() {
                                    attempt = 1; // Reset attempt for new key!
                                    continue;
                                } else {
                                    self.tracker.lock().unwrap().mark_daily(&provider.name);
                                    last_error = Some(e);
                                    self.active_index.store((idx + 1) % self.providers.len(), std::sync::atomic::Ordering::SeqCst);
                                    break;
                                }
                            }
                            ErrorKind::RpmLimit => {
                                if rpm_waits >= 3 {
                                    println!("   ⚠️  RPM limit persists → marking key as exhausted");
                                    let mut pool = provider.key_pool.lock().unwrap();
                                    pool.mark_exhausted();
                                    if pool.has_available() {
                                        attempt = 1;
                                        rpm_waits = 0;
                                        continue;
                                    } else {
                                        self.tracker.lock().unwrap().mark_daily(&provider.name);
                                        last_error = Some(e);
                                        self.active_index.store((idx + 1) % self.providers.len(), std::sync::atomic::Ordering::SeqCst);
                                        break;
                                    }
                                }
                                rpm_waits += 1;
                                self.tracker.lock().unwrap().mark_rpm(&provider.name, 30);
                                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                                continue;
                            }
                            ErrorKind::Other => {
                                last_error = Some(e);
                                if attempt < 3 {
                                    attempt += 1;
                                    tokio::time::sleep(tokio::time::Duration::from_secs(2_u64.pow(attempt as u32))).await;
                                    continue;
                                } else {
                                    println!("   ❌ {} failed after 3 attempts", provider.name);
                                    self.active_index.store((idx + 1) % self.providers.len(), std::sync::atomic::Ordering::SeqCst);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("All providers exhausted")))
    }

    fn mode(&self) -> &'static str {
        "live"
    }

    fn get_stats(&self) -> LlmCallStats {
        self.stats.lock().unwrap().clone()
    }
}

impl LiveProvider {
    async fn try_call(&self, provider: &Provider, req: &LLMRequest) -> Result<LLMResponse> {
        let client = reqwest::Client::new();

        let mut messages = vec![Message {
            role: "system".to_string(),
            content: req.system.clone(),
        }];

        for msg in &req.messages {
            messages.push(Message {
                role: msg.role.clone(),
                content: msg.content.clone(),
            });
        }

        let mut body = RequestBody {
            model: provider.model.clone(),
            messages,
            temperature: req.temperature,
            seed: req.seed,
            response_format: None,
        };

        // JSON mode لـ Planning فقط
        if req.model.contains("plan") || req.system.contains("SCHEMA") {
            body.response_format = Some(serde_json::json!({ "type": "json_object" }));
        }

        // Gemini لا يدعم seed
        if provider.name == "Gemini" {
            body.seed = None;
        }

        // GitHub لا يدعم response_format
        if provider.name == "GitHub" {
            body.response_format = None;
        }

        let api_key = {
            let mut pool = provider.key_pool.lock().unwrap();
            pool.next_available().unwrap_or_default().to_string()
        };

        let resp = client
            .post(&provider.endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, body));
        }

        let data: Response = resp.json().await?;

        let choice = data.choices.into_iter().next()
            .ok_or_else(|| anyhow!("Empty response"))?;

        Ok(LLMResponse {
            content: choice.message.content,
            tokens_in: data.usage.prompt_tokens,
            tokens_out: data.usage.completion_tokens,
            finish_reason: choice.finish_reason,
            provider_model: Some(provider.model.clone()),
            provider_used: provider.name.clone(),
            task_kind: req.model.clone(),
        })
    }
}

impl LiveProvider {
    pub fn print_info(&self) {
        println!("🔗 Providers: {} configured", self.providers.len());
        for (i, p) in self.providers.iter().enumerate() {
            let badge = if i == 0 { "🟢" } else { "⚪" };
            println!("   {} {}: {}", badge, p.name, p.key_preview());
        }

        if self.providers.len() > 1 {
            let fallbacks: Vec<&str> = self.providers[1..].iter().map(|p| p.name.as_str()).collect();
            println!("   🔄 Fallback:  {}", fallbacks.join(" → "));
        }

        if let Some(p) = self.providers.first() {
            println!("   🔌 Provider:  {}", p.name);
            println!("   🤖 Model:     {}", p.model);
        }
    }
}
