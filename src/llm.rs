use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use crate::types::Message;

const SYSTEM_PROMPT: &str = include_str!("system_prompt.txt");
// ─── Gemini API Types ─────────────────────────────────────────────────────────
#[derive(serde::Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiConfig,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct GeminiPart {
    text: String,
}

#[derive(serde::Serialize)]
struct GeminiConfig {
    temperature: f32,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
}

#[derive(serde::Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(serde::Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}


const SYSTEM_PROMPT_COMPACT: &str = include_str!("system_prompt_compact.txt");

fn select_prompt(model: &str) -> &'static str {
    // نماذج Groq: TPM محدود — استخدم النسخة المضغوطة
    let endpoint = std::env::var("SEL_API_BASE").unwrap_or_default();
    if endpoint.contains("groq.com") {
        return SYSTEM_PROMPT_COMPACT;
    }
    // نماذج OpenRouter المجانية: نافذة صغيرة
    if model.contains(":free") {
        return SYSTEM_PROMPT_COMPACT;
    }
    // Gemini free tier — استخدم المضغوط
    if model.contains("flash-lite") || model.contains("gemini-2.0-flash") || model.contains("gemini-2.5-flash") {
        return SYSTEM_PROMPT_COMPACT;
    }
    // النماذج المدفوعة أو الكبيرة: النسخة الكاملة
    SYSTEM_PROMPT
}

#[derive(Debug, Clone, Default)]
pub struct LlmCallStats {
    pub retries: u32,
    pub connection_errors: u32,
    pub rate_limits: u32,
    pub timeouts: u32,
    pub total_latency_ms: u64,
}



// ─── v7.0: Multi-Provider Support ─────────────────────────────────────────────

/// v8.3: تطبيق --provider و --model مباشرة
pub fn apply_provider(provider: Option<&str>, model: Option<&str>) {
    // --model له أولوية قصوى
    if let Some(m) = model {
        std::env::set_var("SEL_MODEL", m);
        eprintln!("🎯 Model: {}", m);
    }

    if let Some(p) = provider {
        match p {
            "gemini" => {
                eprintln!("🔷 Provider: Gemini");
                // GEMINI_API_KEY يجب أن يكون موجوداً
                if std::env::var("GEMINI_API_KEY").is_err() {
                    eprintln!("⚠️  GEMINI_API_KEY not set!");
                }
            }
            "groq" => {
                eprintln!("🟢 Provider: Groq");
                // أوقف Gemini لإجبار استخدام Groq
                std::env::remove_var("GEMINI_API_KEY");
                std::env::set_var("SEL_API_BASE", "https://api.groq.com/openai/v1");
                if model.is_none() {
                    std::env::set_var("SEL_MODEL", "llama-3.3-70b-versatile");
                }
            }
            "openrouter" | "or" => {
                eprintln!("🔵 Provider: OpenRouter");
                std::env::remove_var("GEMINI_API_KEY");
                std::env::set_var("SEL_API_BASE", "https://openrouter.ai/api/v1");
                if model.is_none() {
                    std::env::set_var("SEL_MODEL", "qwen/qwen3.6-plus:free");
                }
            }
            "deepseek" => {
                eprintln!("🔴 Provider: DeepSeek (via OpenRouter)");
                std::env::remove_var("GEMINI_API_KEY");
                std::env::set_var("SEL_API_BASE", "https://openrouter.ai/api/v1");
                std::env::set_var("SEL_MODEL", "deepseek/deepseek-chat-v3-0324");
            }
            "qwen" => {
                eprintln!("🟡 Provider: Qwen (via OpenRouter)");
                std::env::remove_var("GEMINI_API_KEY");
                std::env::set_var("SEL_API_BASE", "https://openrouter.ai/api/v1");
                std::env::set_var("SEL_MODEL", "qwen/qwen3.6-plus:free");
            }
            "nemotron" | "nvidia" => {
                eprintln!("🟢 Provider: Nvidia Nemotron (via OpenRouter)");
                std::env::remove_var("GEMINI_API_KEY");
                std::env::set_var("SEL_API_BASE", "https://openrouter.ai/api/v1");
                std::env::set_var("SEL_MODEL", "nvidia/nemotron-3-nano-30b-a3b:free");
            }
            "gemma" => {
                eprintln!("🔵 Provider: Google Gemma (via OpenRouter)");
                std::env::remove_var("GEMINI_API_KEY");
                std::env::set_var("SEL_API_BASE", "https://openrouter.ai/api/v1");
                std::env::set_var("SEL_MODEL", "google/gemma-3-12b-it:free");
            }
            _ => {
                eprintln!("⚠️  Unknown provider '{}'. Use: groq | openrouter | gemini | deepseek | qwen", p);
            }
        }
    }
}

/// Priority: SEL_API_KEY → OPENROUTER_API_KEY → GROQ_API_KEY
pub fn resolve_api_key() -> String {
    // v8.2: Gemini فقط إذا لم يكن SEL_API_BASE مضبوطاً (SEL_MODEL له الأولوية)
    let has_explicit_provider = std::env::var("SEL_API_BASE")
        .map(|b| !b.trim().is_empty())
        .unwrap_or(false);
    if !has_explicit_provider {
        if let Ok(k) = std::env::var("GEMINI_API_KEY") {
            if !k.trim().is_empty() { eprintln!("🔑 Using GEMINI_API_KEY"); return k; }
        }
    }
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
        if !k.trim().is_empty() { 
            eprintln!("🔑 Using GROQ_API_KEY"); 
            std::env::set_var("SEL_API_BASE", "https://api.groq.com/openai/v1");
            return k; 
        }
    }
    if let Ok(k) = std::env::var("OPENROUTER_API_KEY") {
        if !k.trim().is_empty() { 
            eprintln!("🔑 Using OPENROUTER_API_KEY"); 
            std::env::set_var("SEL_API_BASE", "https://openrouter.ai/api/v1");
            // Use Nemotron (free, available)
            if std::env::var("SEL_MODEL").is_err() {
                std::env::set_var("SEL_MODEL", "nvidia/nemotron-3-nano-30b-a3b:free");
            }
            return k; 
        }
    }
    panic!("❌ No API key found. Set GROQ_API_KEY or OPENROUTER_API_KEY");
}

/// Priority: SEL_MODEL → default kimi-k2
pub fn resolve_model() -> String {
    if let Ok(m) = std::env::var("SEL_MODEL") {
        if !m.trim().is_empty() { return m; }
    }
    // Auto-select based on provider
    if std::env::var("OPENROUTER_API_KEY").is_ok() {
        return "deepseek/deepseek-chat".to_string();
    }
    if std::env::var("GROQ_API_KEY").is_ok() {
        return "llama-3.3-70b-versatile".to_string();
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
        let model_id = model.clone();
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
struct Choice { message: RespMsg }

#[derive(Deserialize)]
struct RespMsg { role: Option<String>, content: Option<String> }

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
        let model_id = model.clone();
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

    /// استدعاء Gemini API مباشرة
    pub async fn gemini_call(&self, messages: &[Message]) -> Result<(String, LlmCallStats)> {
        let mut stats = LlmCallStats::default();
        let call_start = std::time::Instant::now();

        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| anyhow::anyhow!("GEMINI_API_KEY not set"))?;

        let model = std::env::var("GEMINI_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                if self.model.starts_with("gemini") {
                    Some(self.model.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "gemini-2.0-flash-lite".to_string());

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, api_key
        );

        let system_content = select_prompt(&self.model).to_string();
        let system_instruction = GeminiContent {
            parts: vec![GeminiPart { text: system_content }],
            role: None,
        };

        let contents: Vec<GeminiContent> = messages.iter().map(|m| GeminiContent {
            parts: vec![GeminiPart { text: m.content.clone() }],
            role: Some(if m.role == "assistant" { "model".to_string() } else { "user".to_string() }),
        }).collect();

        let request = GeminiRequest {
            contents,
            system_instruction: Some(system_instruction),
            generation_config: GeminiConfig {
                temperature: 0.1,
                max_output_tokens: 8192,
            },
        };

        // v8.1: فحص حجم المدخلات — قبل بناء request
        let total_chars: usize = messages.iter().map(|m| m.content.len()).sum::<usize>()
            + select_prompt(&self.model).len();
        let estimated_tokens = total_chars / 4;
        eprintln!("   📏 Gemini input: ~{} tokens", estimated_tokens);
        if estimated_tokens > 12_000 {
            eprintln!("   ⚠️  Large prompt ({} tokens) — may hit rate limits", estimated_tokens);
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let delays = [15u64, 45, 120];
        for (attempt, &delay) in delays.iter().enumerate() {
            if attempt > 0 {
                stats.retries += 1;
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }

            let resp = match client.post(&url).json(&request).send().await {
                Ok(r) => r,
                Err(e) => {
                    stats.connection_errors += 1;
                    if attempt + 1 == delays.len() {
                        stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
                        return Err(anyhow::anyhow!("Gemini connection error: {}", e));
                    }
                    println!("   ⚠ Gemini connection error — retry in {}s...", delay);
                    continue;
                }
            };

            let status = resp.status();

            if status.as_u16() == 429 {
                stats.rate_limits += 1;
                let body = resp.text().await.unwrap_or_default();
                // Daily limit
                if body.contains("limit: 0") || body.contains("RESOURCE_EXHAUSTED") {
                    stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
                    return Err(anyhow::anyhow!("Gemini daily limit reached"));
                }
                if attempt + 1 == delays.len() {
                    return Err(anyhow::anyhow!("Gemini 429: {}", &body[..body.len().min(200)]));
                }
                println!("   ⚠ Gemini rate limit — retry in {}s...", delay);
                continue;
            }

            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!("Gemini {} — {}", status, &body[..body.len().min(200)]));
            }

            let data: GeminiResponse = resp.json().await
                .map_err(|e| anyhow::anyhow!("Gemini parse error: {}", e))?;

            stats.total_latency_ms = call_start.elapsed().as_millis() as u64;

            let text = data.candidates.into_iter().next()
                .and_then(|c| c.content.parts.into_iter().next())
                .map(|p| p.text)
                .ok_or_else(|| anyhow::anyhow!("Gemini empty response"))?;

            return Ok((text, stats));
        }

        Err(anyhow::anyhow!("Gemini failed after all retries"))
    }

    pub async fn call(&self, messages: &[Message]) -> Result<(String, LlmCallStats)> {
        // v8.2: إذا كان api_key من Gemini — استخدم gemini_call
        if std::env::var("GEMINI_API_KEY")
            .map(|k| !k.is_empty() && self.api_key == k)
            .unwrap_or(false) {
            return self.gemini_call(messages).await;
        }
        let mut stats      = LlmCallStats::default();
        let call_start     = std::time::Instant::now();
        let system_content = select_prompt(&self.model).to_string();
        let mut msgs = vec![ApiMsg { role: "system".into(), content: system_content }];
        for m in messages { msgs.push(ApiMsg { role: m.role.clone(), content: m.content.clone() }); }

        // v8.1: فحص حجم المدخلات
        let total_input_chars: usize = msgs.iter().map(|m| m.content.len()).sum();
        let estimated_tokens = total_input_chars / 4;
        eprintln!("   📏 Input: ~{} tokens", estimated_tokens);
        if estimated_tokens > 12_000 {
            eprintln!("   ⚠️  Large prompt ({} tokens) — may hit rate limits", estimated_tokens);
        }

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

            let body_text = resp.text().await.unwrap_or_default();
            let data: Response = match serde_json::from_str(&body_text) {
                Ok(d) => d,
                Err(e) => {
                    stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
                    let snippet = if body_text.len() > 1000 { &body_text[..1000] } else { &body_text };
                    return Err(anyhow!("JSON Parse Error: {} — Body: {}", e, snippet));
                }
            };
            stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
            let result = data.choices.into_iter().next()
                .map(|c| c.message.content)
                .ok_or_else(|| anyhow!("Empty response"));
            return result.map(|text| (text.unwrap_or_default(), stats));
        }
        stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
        Err(anyhow!("LLM failed after all retries"))
    }
}
