use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use crate::types::Message;

#[derive(Debug, Clone, Default)]
pub struct LlmCallStats {
    pub retries: u32,
    pub connection_errors: u32,
    pub rate_limits: u32,
    pub timeouts: u32,
    pub total_latency_ms: u64,
}

/// معلومات Provider للعرض
pub fn provider_display_name(endpoint: &str, model: &str) -> String {
    if endpoint.contains("generativelanguage") {
        "Gemini".to_string()
    } else if endpoint.contains("groq") {
        if model.contains("kimi") { "Groq/Kimi".to_string() }
        else { "Groq/Llama".to_string() }
    } else if endpoint.contains("openrouter") {
        "OpenRouter".to_string()
    } else if endpoint.contains("nvidia") {
        "NVIDIA".to_string()
    } else if endpoint.contains("localhost") || endpoint.contains("11434") {
        "Ollama (Local)".to_string()
    } else {
        "Custom".to_string()
    }
}

/// وقت التجديد (UTC midnight)
pub fn time_to_reset() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_to_reset = 86400 - (now % 86400);
    let hours = secs_to_reset / 3600;
    let mins  = (secs_to_reset % 3600) / 60;
    format!("{}h {}m", hours, mins)
}

/// حد tokens اليومي حسب Provider
pub fn daily_limit(endpoint: &str, model: &str) -> String {
    if endpoint.contains("generativelanguage") {
        "1,000,000 tokens/day (free)".to_string()
    } else if endpoint.contains("groq") {
        if model.contains("kimi") {
            "300,000 tokens/day".to_string()
        } else {
            "500,000 tokens/day".to_string()
        }
    } else if endpoint.contains("openrouter") {
        "مدفوع — بلا حد يومي".to_string()
    } else if endpoint.contains("localhost") {
        "محلي — بلا حد".to_string()
    } else {
        "غير معروف".to_string()
    }
}

/// رسالة خطأ API واضحة
pub fn classify_api_error(status: u16, body: &str, provider: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_to_reset = 86400 - (now % 86400);
    let h = secs_to_reset / 3600;
    let m = (secs_to_reset % 3600) / 60;

    match status {
        429 if body.contains("tokens per day") || body.contains("TPD") =>
            format!("❌ [{}] نفد الحد اليومي — يتجدد بعد {}h {}m", provider, h, m),
        429 if body.contains("tokens per minute") || body.contains("TPM") =>
            format!("⚠️  [{}] حد الطلبات/دقيقة وصل", provider),
        429 =>
            format!("⚠️  [{}] حد الطلبات وصل (429)", provider),
        402 =>
            format!("❌ [{}] رصيد منتهٍ — openrouter.ai/credits", provider),
        413 =>
            format!("❌ [{}] الطلب كبير جداً (413) — context ضخم", provider),
        503 | 502 | 500 =>
            format!("⚠️  [{}] الخادم مشغول مؤقتاً ({})", provider, status),
        401 =>
            format!("❌ [{}] مفتاح API خاطئ (401)", provider),
        403 =>
            format!("❌ [{}] لا صلاحية لهذا النموذج (403)", provider),
        _ =>
            format!("❌ [{}] خطأ HTTP {} — {}", provider, status, &body[..body.len().min(60)]),
    }
}

/// رسالة خطأ شبكة واضحة
pub fn classify_network_error(err: &str) -> String {
    if err.contains("timed out") || err.contains("timeout") {
        "⚠️  [الشبكة] انتهت مهلة الاتصال".to_string()
    } else if err.contains("connection refused") {
        "❌ [الشبكة] رُفض الاتصال — هل الخادم يعمل؟".to_string()
    } else if err.contains("dns") || err.contains("resolve") {
        "❌ [الشبكة] فشل DNS — تحقق من الإنترنت".to_string()
    } else {
        format!("⚠️  [الشبكة] انقطع الاتصال — {}", &err[..err.len().min(50)])
    }
}

/// رسالة خطأ JSON واضحة
pub fn classify_json_error(reason: &str) -> String {
    if reason.contains("No ```json") || reason.contains("json block") {
        "⚠️  [النموذج] رد بنص بدل JSON — إعادة بـ prompt مبسط".to_string()
    } else if reason.contains("missing field") {
        "⚠️  [النموذج] JSON ناقص حقل مطلوب".to_string()
    } else {
        format!("⚠️  [النموذج] فشل تحليل JSON — {}", &reason[..reason.len().min(50)])
    }
}

/// طباعة معلومات Provider
pub fn print_provider_info(endpoint: &str, model: &str, api_key: &str) {
    let provider = provider_display_name(endpoint, model);
    let key_short = if api_key.len() > 12 {
        format!("{}...{}", &api_key[..8], &api_key[api_key.len()-4..])
    } else {
        "****".to_string()
    };
    let limit = daily_limit(endpoint, model);
    let reset = time_to_reset();

    println!("   🔌 Provider:  {}", provider);
    println!("   🤖 Model:     {}", model);
    println!("   🔑 Key:       {}", key_short);
    println!("   📊 Limit:     {}", limit);
    println!("   ⏰ Resets in: {} (UTC midnight)", reset);
}



const SYSTEM_PROMPT: &str = r#"You are SEL Agent — a deterministic software execution agent.
OUTPUT: Respond ONLY with a single json code block. No text outside it.

SCHEMA:
{"version":"1.0","commands":[
  {"type":"run","command":"python3 -m venv venv"},
  {"type":"run","command":"venv/bin/pip3 install pytest"},
  {"type":"write_file","path":"calc.py","content":"def add(a,b):\n    return a+b\n"},
  {"type":"write_file","path":"test_calc.py","content":"from calc import add\ndef test_add():\n    assert add(1,2)==3\n"},
  {"type":"run_tests","target":"test_calc.py"},
  {"type":"done","message":"All tests passed"}
]}

CRITICAL RULES:
- Python: venv/bin/pytest, never name files math.py/os.py/json.py/csv.py
- Rust: double quotes for strings, no cargo new, Cargo.toml at workspace root
  run_tests target MUST be "cargo" → {"type":"run_tests","target":"cargo"}
- Go: go.mod required, Test prefix, _test.go suffix, t.Errorf for assertions
  run_tests target MUST be "go" → {"type":"run_tests","target":"go"}
  ALWAYS add import "fmt" at top if using fmt.Sprintf, fmt.Errorf, or fmt.Println
  Go imports example: import (
    "fmt"
    "errors"
  )
  ALWAYS import "errors" if using errors.New
  String literals in Go use double quotes: "hello" not 'hello'
  Test helper: t.Errorf("got %v, want %v", got, want)
- Node/TS: ALWAYS use {"type":"run_tests","target":"npm test"} — NEVER "node file.ts"
  NEVER use {"type":"run","command":"npm test"} — MUST be run_tests not run
  TypeScript files need Jest via npm test, not direct node execution
- NEVER repeat workspace path in file paths
- ALWAYS include type field in every command
- TESTS MUST BE STRONG: write 3+ assertions per function, test edge cases
  BAD:  assert add(1,2) == 3
  GOOD: assert add(1,2)==3; assert add(0,0)==0; assert add(-1,1)==0; assert add(10,5)==15
- FACTORIAL TESTS: MUST test n=0 (→1), n=1 (→1), n=5 (→120) — all three required
- BOOLEAN TESTS: MUST test both True AND False return values
- RECURSIVE TESTS: MUST test base case AND recursive case
"#;

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    Groq,
    Moonshot,
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub provider: Provider,
    pub model_id: String,
    pub base_url: String,
    pub env_key: String,
}

impl ModelConfig {
    pub fn from_alias(alias: &str) -> Self {
        match alias {
            "kimi" | "kimi-k2" | "kimi-k2-instruct" => ModelConfig {
                provider: Provider::Groq,
                model_id: "moonshotai/kimi-k2-instruct-0905".to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key: "GROQ_API_KEY".to_string(),
            },
            "llama" | "llama-70b" => ModelConfig {
                provider: Provider::Groq,
                model_id: "llama-3.3-70b-versatile".to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key: "GROQ_API_KEY".to_string(),
            },
            "kimi-k2.5" | "kimi25" | "kimi-latest" => ModelConfig {
                provider: Provider::Moonshot,
                model_id: "kimi-k2.5".to_string(),
                base_url: "https://api.moonshot.ai/v1/chat/completions".to_string(),
                env_key: "MOONSHOT_API_KEY".to_string(),
            },
            "silicon" | "kimi-silicon" => ModelConfig {
                provider: Provider::Moonshot,
                model_id: "moonshotai/Kimi-K2.5".to_string(),
                base_url: "https://api.siliconflow.cn/v1/chat/completions".to_string(),
                env_key: "SILICONFLOW_API_KEY".to_string(),
            },
            _ => ModelConfig {
                provider: Provider::Groq,
                model_id: alias.to_string(),
                base_url: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                env_key: "GROQ_API_KEY".to_string(),
            },
        }
    }
}

pub struct LlmClient { pub api_key: String, pub model: String, pub endpoint: String, pub config: ModelConfig }

#[derive(Serialize)]
struct Request { model: String, messages: Vec<ApiMsg>, temperature: f32, max_tokens: u32 }

#[derive(Serialize, Deserialize, Clone)]
struct ApiMsg { role: String, content: String }

#[derive(Deserialize)]
struct Response { choices: Vec<Choice> }

#[derive(Deserialize)]
struct Choice { message: ApiMsg }

impl LlmClient {
    pub fn new(api_key: String) -> Self {
        let config = ModelConfig::from_alias("kimi");
        Self {
            api_key,
            model:    config.model_id.clone(),
            endpoint: config.base_url.clone(),
            config,
        }
    }

    pub fn with_model(alias: &str) -> Self {
        let config = ModelConfig::from_alias(alias);
        let api_key = std::env::var(&config.env_key)
            .unwrap_or_else(|_| panic!("❌ متغير البيئة {} غير موجود", config.env_key));
        Self {
            model:    config.model_id.clone(),
            endpoint: config.base_url.clone(),
            api_key,
            config,
        }
    }

    pub async fn call(&self, messages: &[Message]) -> Result<(String, LlmCallStats)> {
        let mut stats = LlmCallStats::default();
        let call_start = std::time::Instant::now();
        let mut msgs = vec![ApiMsg { role: "system".into(), content: SYSTEM_PROMPT.into() }];
        for m in messages { msgs.push(ApiMsg { role: m.role.clone(), content: m.content.clone() }); }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()?;
        let delays = [15u64, 45, 120];

        for (attempt, &delay) in delays.iter().enumerate() {
            if attempt > 0 {
                stats.retries += 1;
                println!("   ⏳ retry in {}s...", delay);
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }

            let resp = match client
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(&Request {
                    model: self.model.clone(),
                    messages: msgs.clone(),
                    temperature: 0.1,
                    max_tokens: 8192,
                })
                .send().await {
                    Ok(r)  => r,
                    Err(e) => {
                        stats.connection_errors += 1;
                        if attempt + 1 == delays.len() {
                            stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
                            return Err(anyhow!("Connection error: {}", e));
                        }
                        println!("   {} — retry in {}s...", classify_network_error(&e.to_string()), delay);
                        continue;
                    }
                };

            let status = resp.status();
            if status == 429 || status == 503 || status == 502 || status == 500 {
                if status == 429 { stats.rate_limits += 1; } else { stats.connection_errors += 1; }
                if attempt + 1 == delays.len() {
                    let body = resp.text().await.unwrap_or_default();
                    stats.total_latency_ms = call_start.elapsed().as_millis() as u64;
                    return Err(anyhow!("API {} — {}", status, &body[..body.len().min(200)]));
                }
                let label = if status == 429 { "Rate limit" } else { "Server error" };
                println!("   ⚠ {} ({}) — retry in {}s...", label, status, delay);
                if attempt > 0 { stats.retries += 1; }
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
        Err(anyhow!("LLM failed"))
    }
}
