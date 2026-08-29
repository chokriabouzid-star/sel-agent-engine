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
            model: std::env::var("CEREBRAS_MODEL").unwrap_or_else(|_| "llama-3.3-70b".into()),
            endpoint: "https://api.cerebras.ai/v1/chat/completions".into(),
            key_pool: Arc::new(Mutex::new(super::key_pool::KeyPool::from_env(
                "CEREBRAS_API_KEY",
            ))),
        }
    }

    fn gemini() -> Self {
        Provider {
            name: "Gemini".into(),
            model: std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.0-flash".into()),
            endpoint: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
                .into(),
            key_pool: Arc::new(Mutex::new(super::key_pool::KeyPool::from_env(
                "GEMINI_API_KEY",
            ))),
        }
    }

    fn groq() -> Self {
        Provider {
            name: "Groq".into(),
            model: std::env::var("GROQ_MODEL").unwrap_or_else(|_| "openai/gpt-oss-120b".into()),
            endpoint: "https://api.groq.com/openai/v1/chat/completions".into(),
            key_pool: Arc::new(Mutex::new(super::key_pool::KeyPool::from_env(
                "GROQ_API_KEY",
            ))),
        }
    }

    fn openrouter() -> Self {
        let mut pool = super::key_pool::KeyPool::from_env("OPENROUTER_API_KEY");
        if !pool.has_available() {
            pool = super::key_pool::KeyPool::from_env("SEL_API_KEY");
        }
        Provider {
            name: "OpenRouter".into(),
            model: std::env::var("OPENROUTER_MODEL")
                .unwrap_or_else(|_| "qwen/qwen3-coder:free".into()),
            endpoint: "https://openrouter.ai/api/v1/chat/completions".into(),
            key_pool: Arc::new(Mutex::new(pool)),
        }
    }

    #[allow(dead_code)]
    fn is_configured(&self) -> bool {
        self.key_pool
            .lock()
            .map(|p| p.has_available())
            .unwrap_or(false)
    }

    fn key_preview(&self) -> String {
        let mut pool = match self.key_pool.lock() {
            Ok(p) => p,
            Err(_) => return "<lock-err>".to_string(),
        };
        let key_opt = pool.next_available();
        if let Some(k) = key_opt {
            if k.len() > 8 {
                format!("{}...{}", safe_prefix_chars(k, 4), safe_suffix_chars(k, 2))
            } else if k.len() > 4 {
                format!("{}...", safe_prefix_chars(k, 3))
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

fn safe_prefix_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn safe_suffix_chars(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let start = chars.len().saturating_sub(n);
    chars[start..].iter().collect()
}

fn parse_leading_status_code(s: &str) -> Option<u16> {
    let code: String = s.chars().take(3).collect();
    if code.len() == 3 && code.chars().all(|c| c.is_ascii_digit()) {
        code.parse().ok()
    } else {
        None
    }
}

impl LiveProvider {
    pub fn from_env() -> Self {
        Self::new()
    }

    pub fn primary_name(&self) -> String {
        self.providers
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_default()
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
            // ("GITHUB_TOKEN", Provider::github()),
        ];

        for (env_name, p) in candidates {
            let pool = match p.key_pool.lock() {
                Ok(g) => g,
                Err(_) => continue,
            };
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
    DailyLimit,       // HTTP 403 quota, 86400s exceeded
    RpmLimit,         // HTTP 429 rate limit
    KeyExpired, // explicit evidence only: api_key_invalid / api key expired / api key not valid / invalid_api_key
    ProviderRejected, // deterministic reject: HTTP 400/404, or 401 WITHOUT explicit key-invalidity evidence — NOT proof of a dead key or exhausted quota
    Other,
}

fn classify_error(err: &str) -> ErrorKind {
    let lower = err.to_lowercase();

    // Parse HTTP status if present (e.g. "HTTP 400 Bad Request: ...")
    let mut status = 0;
    if let Some(idx) = err.find("HTTP ") {
        let rest = &err[idx + 5..];
        if rest.len() >= 3 {
            if let Some(s) = parse_leading_status_code(rest) {
                status = s;
            }
        }
    }

    // KeyExpired: EXPLICIT key-invalidity evidence only. Narrowed 2026 — a bare
    // "unauthorized" or bare HTTP 401 alone is NOT sufficient proof the key
    // itself is dead (e.g. "401 User not found" is a provider-side rejection,
    // not a confirmed dead key). See ErrorKind::ProviderRejected below for that.
    let is_key_error_msg = lower.contains("api_key_invalid")
        || lower.contains("api key expired")
        || lower.contains("api key not valid")
        || lower.contains("api key e")
        || lower.contains("invalid_api_key")
        || lower.contains("wrong api key")
        || lower.contains("please pass a valid api key")
        || lower.contains("user not found");

    if is_key_error_msg {
        return ErrorKind::KeyExpired;
    }

    if lower.contains("per 86400s exceeded")
        || lower.contains("generaterequestsperdayperproject")
        || lower.contains("free_tier_requests")
        || lower.contains("quota")
        || lower.contains("daily limit")
        || lower.contains("per day")
        || lower.contains("payment required")
        || lower.contains("payment_required")
        || (status == 403 && lower.contains("quota"))
        || status == 402
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

    // ProviderRejected: deterministic rejection that will NOT succeed on retry,
    // and is NOT proof of a dead key or exhausted quota:
    // - HTTP 400 (request rejected as currently formed for this provider)
    // - HTTP 404 / "not_found" (wrong model/endpoint)
    // - HTTP 401 that reached here WITHOUT matching an explicit key-invalidity
    //   phrase above (e.g. "User not found")
    if status == 400 || status == 404 || status == 401 || lower.contains("not_found") {
        return ErrorKind::ProviderRejected;
    }

    ErrorKind::Other
}

#[async_trait::async_trait]
impl LLMProvider for LiveProvider {
    async fn complete(&self, req: LLMRequest) -> Result<LLMResponse> {
        let mut last_error = None;
        let provider_names: Vec<String> = self.providers.iter().map(|p| p.name.clone()).collect();

        {
            let tracker = match self.tracker.lock() {
                Ok(g) => g,
                Err(e) => return Err(anyhow!("tracker lock poisoned: {}", e)),
            };
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
                    let tracker_available = {
                        match self.tracker.lock() {
                            Ok(t) => t.is_available(&provider.name),
                            Err(_) => false,
                        }
                    };
                    let has_keys = {
                        match provider.key_pool.lock() {
                            Ok(p) => p.has_available(),
                            Err(_) => false,
                        }
                    };
                    if !tracker_available || !has_keys {
                        if !has_keys && attempt == 1 {
                            println!(
                                "   ⏭  Skipping {}  all keys expired/exhausted",
                                provider.name
                            );
                        }
                        self.active_index.store(
                            (idx + 1) % self.providers.len(),
                            std::sync::atomic::Ordering::SeqCst,
                        );
                        break;
                    }
                }

                let is_primary = idx == 0;
                let prefix = if is_primary { "📡" } else { "🔀" };
                if attempt == 1 {
                    println!("{} Calling {} ({})", prefix, provider.name, provider.model);
                } else {
                    let err_short: String = last_error
                        .as_ref()
                        .map(|e: &anyhow::Error| e.to_string().chars().take(80).collect::<String>())
                        .unwrap_or_default();
                    println!(
                        "   🔄 Attempt {}/3 [{}] - retrying {}...",
                        attempt, err_short, provider.name
                    );
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
                                let mut pool = match provider.key_pool.lock() {
                                    Ok(p) => p,
                                    Err(_) => break,
                                };
                                pool.mark_expired(); // permanently removes it
                                if pool.has_available() {
                                    attempt = 1;
                                    continue;
                                } else {
                                    if let Ok(mut t) = self.tracker.lock() {
                                        t.mark_daily(&provider.name);
                                    }
                                    last_error = Some(e);
                                    self.active_index.store(
                                        (idx + 1) % self.providers.len(),
                                        std::sync::atomic::Ordering::SeqCst,
                                    );
                                    break;
                                }
                            }
                            ErrorKind::DailyLimit => {
                                let mut pool = match provider.key_pool.lock() {
                                    Ok(p) => p,
                                    Err(_) => break,
                                };
                                pool.mark_exhausted();
                                if pool.has_available() {
                                    attempt = 1; // Reset attempt for new key!
                                    continue;
                                } else {
                                    if let Ok(mut t) = self.tracker.lock() {
                                        t.mark_daily(&provider.name);
                                    }
                                    last_error = Some(e);
                                    self.active_index.store(
                                        (idx + 1) % self.providers.len(),
                                        std::sync::atomic::Ordering::SeqCst,
                                    );
                                    break;
                                }
                            }
                            ErrorKind::ProviderRejected => {
                                // Try other keys in THIS provider's pool first
                                // (mirrors KeyExpired/DailyLimit pattern) before
                                // giving up on the whole provider. Only the
                                // rejected key is skipped, never persisted.
                                let mut pool = match provider.key_pool.lock() {
                                    Ok(p) => p,
                                    Err(_) => break,
                                };
                                pool.mark_rejected_this_run();
                                if pool.has_available() {
                                    attempt = 1;
                                    continue;
                                } else {
                                    if let Ok(mut t) = self.tracker.lock() {
                                        t.mark_rejected(&provider.name);
                                    }
                                    last_error = Some(e);
                                    self.active_index.store(
                                        (idx + 1) % self.providers.len(),
                                        std::sync::atomic::Ordering::SeqCst,
                                    );
                                    break;
                                }
                            }
                            ErrorKind::RpmLimit => {
                                rpm_waits += 1;
                                // v8.0:     RPM
                                //  3    provider   (  )
                                if rpm_waits >= 3 {
                                    println!("   ⚠️  RPM limit persists  skipping {} temporarily (key preserved)", provider.name);
                                    if let Ok(mut t) = self.tracker.lock() {
                                        t.mark_rpm(&provider.name, 60);
                                    }
                                    last_error = Some(e);
                                    self.active_index.store(
                                        (idx + 1) % self.providers.len(),
                                        std::sync::atomic::Ordering::SeqCst,
                                    );
                                    break; //   provider   mark_exhausted
                                }
                                {
                                    if let Ok(mut t) = self.tracker.lock() {
                                        t.mark_rpm(&provider.name, 30);
                                    }
                                } // lock dropped before .await
                                println!("   ⏳ [{}] RPM limit  cooling 30s", provider.name);
                                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                                continue;
                            }
                            ErrorKind::Other => {
                                last_error = Some(e);
                                if attempt < 3 {
                                    attempt += 1;
                                    tokio::time::sleep(tokio::time::Duration::from_secs(
                                        2_u64.pow(attempt as u32),
                                    ))
                                    .await;
                                    continue;
                                } else {
                                    println!("   ❌ {} failed after 3 attempts", provider.name);
                                    self.active_index.store(
                                        (idx + 1) % self.providers.len(),
                                        std::sync::atomic::Ordering::SeqCst,
                                    );
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
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl LiveProvider {
    async fn try_call(&self, provider: &Provider, req: &LLMRequest) -> Result<LLMResponse> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .pool_max_idle_per_host(5)
            .build()
            .unwrap_or_default();

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
            let mut pool = match provider.key_pool.lock() {
                Ok(p) => p,
                Err(e) => return Err(anyhow!("key pool lock poisoned: {}", e)),
            };
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

        let choice = data
            .choices
            .into_iter()
            .next()
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
            let fallbacks: Vec<&str> = self.providers[1..]
                .iter()
                .map(|p| p.name.as_str())
                .collect();
            println!("   🔄 Fallback:  {}", fallbacks.join(" → "));
        }

        if let Some(p) = self.providers.first() {
            println!("   🔌 Provider:  {}", p.name);
            println!("   🤖 Model:     {}", p.model);
        }
    }
}

#[cfg(test)]
mod classify_error_tests {
    use super::*;

    #[test]
    fn explicit_key_invalid_message_is_still_key_expired() {
        let err =
            r#"HTTP 400 Bad Request: {"error":"API key not valid. Please pass a valid API key."}"#;
        assert_eq!(classify_error(err), ErrorKind::KeyExpired);
    }

    #[test]
    fn gemini_deterministic_400_is_provider_rejected() {
        // Exact truncated string observed repeated identically across 4
        // independent tasks in the same session, 2026-07-XX
        let err = "HTTP 400 Bad Request: [{\n  \"error\": {\n    \"code\": 400,\n    \"message\": \"Please pa";
        assert_eq!(classify_error(err), ErrorKind::ProviderRejected);
    }

    #[test]
    fn genuine_daily_quota_message_is_still_daily_limit() {
        let err = "HTTP 403 Forbidden: Quota exceeded for quota metric 'GenerateRequestsPerDayPerProjectPerModel'";
        assert_eq!(classify_error(err), ErrorKind::DailyLimit);
    }

    #[test]
    fn genuine_rpm_message_is_still_rpm_limit() {
        let err = "HTTP 429 Too Many Requests: Rate limit reached for requests";
        assert_eq!(classify_error(err), ErrorKind::RpmLimit);
    }

    #[test]
    fn not_found_model_is_provider_rejected_not_daily_limit() {
        let err = "HTTP 404 Not Found: the model does not exist or you do not have access to it";
        assert_eq!(classify_error(err), ErrorKind::ProviderRejected);
    }

    #[test]
    fn generic_transient_network_error_is_still_other() {
        // Exact string observed in logs: transient connection failure
        let err = "error sending request for url (https://api.groq.com/openai/v1/chat/completions)";
        assert_eq!(classify_error(err), ErrorKind::Other);
    }

    #[test]
    fn wrong_api_key_is_key_expired() {
        let err = r#"HTTP 401 Unauthorized: {"message":"Wrong API Key","code":"wrong_api_key"}"#;
        assert_eq!(classify_error(err), ErrorKind::KeyExpired);
    }

    #[test]
    fn valid_api_key_message_is_key_expired() {
        let err = r#"HTTP 400 Bad Request: [{"error":{"code":400,"message":"Please pass a valid API key","status":"INVALID_ARGUMENT"}}]"#;
        assert_eq!(classify_error(err), ErrorKind::KeyExpired);
    }

    #[test]
    fn user_not_found_is_key_expired() {
        let err = r#"HTTP 401 Unauthorized: {"error":{"message":"User not found.","code":401}}"#;
        assert_eq!(classify_error(err), ErrorKind::KeyExpired);
    }

    #[test]
    fn payment_required_is_daily_limit() {
        let err = r#"HTTP 402 Payment Required: {"message":"Payment required to access this resource.","code":"payment_required"}"#;
        assert_eq!(classify_error(err), ErrorKind::DailyLimit);
    }
}
