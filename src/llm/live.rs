// src/llm/live.rs  LiveProvider  SPO
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

impl Default for LiveProvider {
    fn default() -> Self {
        Self::new()
    }
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
        let mut missing_keys = Vec::new();
        let mut exhausted_providers = Vec::new();

        // : Groq  Gemini  Cerebras  OpenRouter  GitHub
        let candidates = vec![
            ("GROQ_API_KEY", Provider::groq()),
            ("GEMINI_API_KEY", Provider::gemini()),
            ("CEREBRAS_API_KEY", Provider::cerebras()),
            ("OPENROUTER_API_KEY", Provider::openrouter()),
            ("GITHUB_TOKEN", Provider::github()),
        ];

        for (env_name, p) in candidates {
            let pool = p.key_pool.lock().unwrap();
            if pool.keys.is_empty() {
                missing_keys.push(env_name);
                continue;
            }
            if !pool.has_available() {
                exhausted_providers.push(p.name.clone());
                continue;
            }
            drop(pool);
            providers.push(p);
        }

        if providers.is_empty() {
            if !missing_keys.is_empty() && exhausted_providers.is_empty() {
                eprintln!(
                    "  ❌ No API keys found in environment. Please set at least one of: {}",
                    missing_keys.join(", ")
                );
            } else if missing_keys.is_empty() && !exhausted_providers.is_empty() {
                eprintln!(
                    "  ⚠️  All configured providers ({}) are currently EXHAUSTED in the cache. \
                     Wait for quota reset or clear ~/.sel-agent/provider_state.json",
                    exhausted_providers.join(", ")
                );
                // We do NOT panic here. We let the provider list be empty.
                // The `complete()` method will return an Err instead, allowing 
                // the bench loop to skip already-recorded cases without crashing.
            } else {
                eprintln!(
                    "  ❌ No LLM provider available. Missing: [{}]. Exhausted: [{}].",
                    missing_keys.join(", "),
                    exhausted_providers.join(", ")
                );
            }
        }

        LiveProvider {
            providers,
            stats: Arc::new(Mutex::new(LlmCallStats::default())),
            active_index: std::sync::atomic::AtomicUsize::new(0),
            tracker: Arc::new(Mutex::new(super::limit_tracker::LimitTracker::new())),
        }
    }

    /// v7.9.6: Clone that SHARES KeyPools, tracker, and stats across tasks
    /// Exhausted keys stay exhausted  no wasted API calls on dead keys
    pub fn clone_shared(&self) -> Self {
        LiveProvider {
            providers: self.providers.clone(), // Provider is Clone  shares Arc<Mutex<KeyPool>>
            stats: self.stats.clone(),
            active_index: std::sync::atomic::AtomicUsize::new(
                self.active_index.load(std::sync::atomic::Ordering::SeqCst),
            ),
            tracker: self.tracker.clone(),
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

#[derive(Debug, PartialEq)]
enum ErrorKind {
    DailyLimit, // HTTP 403 quota, 86400s exceeded
    RpmLimit,   // HTTP 429 rate limit
    KeyExpired, // HTTP 400 API_KEY_INVALID, API key expired
    Other,
}

fn classify_error(err: &str) -> ErrorKind {
    let lower = err.to_lowercase();
    
    // Parse HTTP status if present (e.g. "HTTP 400 Bad Request: ...")
    let mut status = 0;
    if let Some(idx) = err.find("HTTP ") {
        let rest = &err[idx+5..];
        if rest.len() >= 3 {
            if let Ok(s) = rest[..3].parse::<u16>() {
                status = s;
            }
        }
    }
    
    // Check for expired/invalid keys first
    let is_key_error_msg = lower.contains("api_key_invalid") 
        || lower.contains("api key expired") 
        || lower.contains("api key not valid")
        || lower.contains("api key e")
        || lower.contains("invalid_api_key")
        || lower.contains("unauthorized");

    if is_key_error_msg || status == 401 {
        return ErrorKind::KeyExpired;
    }

    if lower.contains("per 86400s exceeded")
        || lower.contains("generaterequestsperdayperproject")
        || lower.contains("free_tier_requests")
        || lower.contains("quota")
        || lower.contains("daily limit")
        || lower.contains("per day")
        || lower.contains("404") 
        || lower.contains("not_found")
        || (status == 403 && lower.contains("quota"))
    {
        return ErrorKind::DailyLimit;
    }
    
    if status == 429
        || lower.contains("per minute")
        || lower.contains("generaterequestsperminuteperproject")
        || lower.contains("generatecontentinputtokenspermodelperminute")
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
                return Err(anyhow!(" All providers exhausted for today"));
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
                    let has_keys = provider.key_pool.lock().unwrap().has_available();
                    if !tracker.is_available(&provider.name) || !has_keys {
                        if !has_keys && attempt == 1 {
                            println!("   ⏭  Skipping {}  all keys expired/exhausted", provider.name);
                        }
                        self.active_index.store((idx + 1) % self.providers.len(), std::sync::atomic::Ordering::SeqCst);
                        break;
                    }
                }
                
                let is_primary = idx == 0;
                let prefix = if is_primary { "📡" } else { "🔀" };
                if attempt == 1 {
                    println!("{} Calling {} ({})", prefix, provider.name, provider.model);
                } else {
                    let err_short: String = last_error.as_ref()
                        .map(|e: &anyhow::Error| e.to_string().chars().take(80).collect::<String>())
                        .unwrap_or_default();
                    println!("   🔄 Attempt {}/3 [{}] - retrying {}...", attempt, err_short, provider.name);
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
                            ErrorKind::KeyExpired => {
                                let mut pool = provider.key_pool.lock().unwrap();
                                pool.mark_expired(); // permanently removes it
                                if pool.has_available() {
                                    attempt = 1;
                                    continue;
                                } else {
                                    self.tracker.lock().unwrap().mark_daily(&provider.name);
                                    last_error = Some(e);
                                    self.active_index.store((idx + 1) % self.providers.len(), std::sync::atomic::Ordering::SeqCst);
                                    break;
                                }
                            }
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
                                rpm_waits += 1;
                                // v8.0:     RPM       
                                //  3    provider   (  )
                                if rpm_waits >= 3 {
                                    println!("   ⚠️  RPM limit persists  skipping {} temporarily (key preserved)", provider.name);
                                    self.tracker.lock().unwrap().mark_rpm(&provider.name, 60);
                                    last_error = Some(e);
                                    self.active_index.store((idx + 1) % self.providers.len(), std::sync::atomic::Ordering::SeqCst);
                                    break; //   provider   mark_exhausted
                                }
                                self.tracker.lock().unwrap().mark_rpm(&provider.name, 30);
                                println!("   ⏳ [{}] RPM limit  cooling 30s", provider.name);
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

        // JSON mode  Planning 
        if req.model.contains("plan") || req.system.contains("SCHEMA") {
            body.response_format = Some(serde_json::json!({ "type": "json_object" }));
        }

        // v7.9.10: Allowlists  providers that don't support seed or JSON response_format
        // Extend this list when adding new providers (e.g. Mistral, Anthropic, SambaNova)
        const SEED_UNSUPPORTED: &[&str] = &["Gemini", "Mistral", "Anthropic"];
        const JSON_MODE_UNSUPPORTED: &[&str] = &["GitHub", "Mistral", "Anthropic"];

        if SEED_UNSUPPORTED.contains(&provider.name.as_str()) {
            body.seed = None;
        }
        if JSON_MODE_UNSUPPORTED.contains(&provider.name.as_str()) {
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
            .timeout(std::time::Duration::from_secs(30)) // v7.9.9 P4: 30s instead of 60s
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
        println!(" 🔗 Providers: {} configured", self.providers.len());
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
