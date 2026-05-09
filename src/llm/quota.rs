// src/llm/quota.rs — SPO v2.1: Quota Manager with atomic persistence
// Updated provider limits based on 2025-2026 field report

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Provider IDs ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    Cerebras,
    Mistral,
    Groq,
    OpenRouter,
    SambaNova,
    Gemini,
    GitHub,
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ProviderId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "cerebras"   => Ok(Self::Cerebras),
            "mistral"    => Ok(Self::Mistral),
            "groq"       => Ok(Self::Groq),
            "openrouter" => Ok(Self::OpenRouter),
            "sambanova"  => Ok(Self::SambaNova),
            "gemini"     => Ok(Self::Gemini),
            "github"     => Ok(Self::GitHub),
            other        => Err(format!("Unknown provider: {}", other)),
        }
    }
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cerebras   => "cerebras",
            Self::Mistral    => "mistral",
            Self::Groq       => "groq",
            Self::OpenRouter => "openrouter",
            Self::SambaNova  => "sambanova",
            Self::Gemini     => "gemini",
            Self::GitHub     => "github",
        }
    }

    pub fn display_order() -> &'static [ProviderId] {
        use ProviderId::*;
        &[Cerebras, Mistral, Groq, OpenRouter, SambaNova, Gemini, GitHub]
    }

    pub fn all() -> Vec<ProviderId> {
        use ProviderId::*;
        vec![Cerebras, Mistral, Groq, OpenRouter, SambaNova, Gemini, GitHub]
    }
}

// ── Provider Limits ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProviderLimits {
    pub daily_requests: Option<u32>,
    pub daily_tokens:   Option<u64>,
    pub rpm:            u32,
    pub default_model:  &'static str,
}

impl ProviderLimits {
    pub fn for_provider(id: &ProviderId) -> Self {
        match id {
            // 1M token/day, no daily req limit. qwen3-235b gives 64K context (vs 8K for llama).
            ProviderId::Cerebras => Self {
                daily_requests: None,
                daily_tokens:   Some(1_000_000),
                rpm:            30,
                default_model:  "qwen3-235b", // 64K context on free tier
            },
            // ⚠️ CRITICAL: RPM=2 is the real bottleneck on free (Experiment) tier.
            // 1B token/month ≈ 33M/day but throughput is 2 req/min max.
            ProviderId::Mistral => Self {
                daily_requests: None,
                daily_tokens:   Some(33_000_000),
                rpm:            2,  // 2 RPM — official Mistral Experiment tier limit
                default_model:  "devstral-small-2507",
            },
            // 1K RPD for 70B (6K TPM is the real bottleneck).
            ProviderId::Groq => Self {
                daily_requests: Some(1_000),
                daily_tokens:   None, // TPM=6K is the actual constraint
                rpm:            30,
                default_model:  "llama-3.3-70b-versatile",
            },
            // Free account = 50 RPD (per-account); per-model :free = up to 200 RPD.
            // Using conservative 50 — bump to 200 if user has $10+ credit.
            ProviderId::OpenRouter => Self {
                daily_requests: Some(50), // conservative: free account global limit
                daily_tokens:   None,
                rpm:            20,
                default_model:  "qwen/qwen3-coder:free",
            },
            // Conservative estimate — no official published daily limit found.
            ProviderId::SambaNova => Self {
                daily_requests: Some(200),
                daily_tokens:   None,
                rpm:            10,
                default_model:  "DeepSeek-V3.2",
            },
            // Flash-Lite = 1,000 RPD / 15 RPM. Flash-2.5 = 250 RPD / 10 RPM.
            // Using Flash-Lite for maximum daily requests.
            ProviderId::Gemini => Self {
                daily_requests: Some(1_000), // gemini-2.0-flash-lite
                daily_tokens:   None,
                rpm:            15,
                default_model:  "gemini-2.0-flash-lite",
            },
            // GPT-4o = 50 RPD | GPT-4o-mini = 150 RPD. Use mini for 3x more capacity.
            ProviderId::GitHub => Self {
                daily_requests: Some(150),
                daily_tokens:   None,
                rpm:            15,
                default_model:  "gpt-4o-mini", // 150 RPD vs 50 for gpt-4o
            },
        }
    }
}

// ── EMA Stats ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskKindStats {
    pub attempts:      u32,
    pub successes:     u32,
    pub total_repairs: u32,
    pub ema_success:   f64,
}

impl TaskKindStats {
    const EMA_ALPHA: f64 = 0.3;

    pub fn update(&mut self, success: bool, repairs: u32) {
        self.attempts      += 1;
        self.total_repairs += repairs;
        if success { self.successes += 1; }
        let outcome = if success { 1.0 } else { 0.0 };
        if self.attempts == 1 {
            self.ema_success = outcome;
        } else {
            self.ema_success = Self::EMA_ALPHA * outcome + (1.0 - Self::EMA_ALPHA) * self.ema_success;
        }
    }

    pub fn avg_repairs(&self) -> f64 {
        if self.attempts == 0 { return 0.0; }
        self.total_repairs as f64 / self.attempts as f64
    }

    pub fn is_statistically_valid(&self) -> bool {
        self.attempts >= 5
    }
}

// ── Provider State ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderState {
    pub id: ProviderId,
    pub requests_today:    u32,
    pub tokens_today:      u64,
    pub requests_this_min: u32,
    pub min_window_start:  u64,
    pub avg_latency_ms:    f64,
    pub consecutive_errors: u32,
    pub json_parse_errors:  u32,
    pub is_exhausted: bool,
    pub cooldown_until: Option<u64>,
    pub reset_at:     u64,
    pub stats_by_kind: HashMap<String, TaskKindStats>,
}

impl ProviderState {
    pub fn new(id: ProviderId) -> Self {
        Self {
            id,
            requests_today: 0,
            tokens_today: 0,
            requests_this_min: 0,
            min_window_start: now_unix(),
            avg_latency_ms: 0.0,
            consecutive_errors: 0,
            json_parse_errors: 0,
            is_exhausted: false,
            cooldown_until: None,
            reset_at: now_unix(),
            stats_by_kind: HashMap::new(),
        }
    }

    pub fn should_reset_daily(&self) -> bool {
        now_unix() >= self.reset_at + 86_400
    }

    pub fn reset_daily(&mut self) {
        self.requests_today     = 0;
        self.tokens_today       = 0;
        self.is_exhausted       = false;
        self.consecutive_errors = 0;
        self.cooldown_until     = None;
        self.reset_at           = now_unix();
    }

    pub fn refresh_minute_window(&mut self) {
        if now_unix() >= self.min_window_start + 60 {
            self.requests_this_min = 0;
            self.min_window_start  = now_unix();
        }
    }

    pub fn update_latency(&mut self, new_ms: f64) {
        const ALPHA: f64 = 0.2;
        if self.avg_latency_ms == 0.0 {
            self.avg_latency_ms = new_ms;
        } else {
            self.avg_latency_ms = ALPHA * new_ms + (1.0 - ALPHA) * self.avg_latency_ms;
        }
    }

    pub fn is_in_cooldown(&self) -> bool {
        if let Some(until) = self.cooldown_until {
            now_unix() < until
        } else {
            false
        }
    }
}

// ── Result Types ─────────────────────────────────────────────

#[derive(Debug)]
pub enum Availability {
    Ready,
    RateLimit { wait_secs: u64 },
    Exhausted,
}

#[derive(Debug)]
pub enum PickResult {
    Selected(ProviderId),
    WaitFor { seconds: u64 },
    AllExhausted,
}

#[derive(Debug, Clone)]
pub enum LlmErrorKind {
    RateLimit,
    DailyExhausted,
    NetworkError,
    JsonParse,
    ContextTooLong,
    AuthError,
    Unknown,
}

// ── QuotaManager ─────────────────────────────────────────────

pub struct QuotaManager {
    states:     HashMap<ProviderId, ProviderState>,
    cache_path: PathBuf,
}

impl QuotaManager {
    pub fn load() -> Self {
        let path = quota_cache_path();
        Self::load_from_path(path)
    }

    pub fn load_from_path(path: PathBuf) -> Self {
        let states = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(Self::default_states)
        } else {
            Self::default_states()
        };
        let mut mgr = Self { states, cache_path: path };
        for id in ProviderId::all() {
            mgr.reset_if_needed(&id);
        }
        mgr
    }

    pub fn save(&self) {
        let json = match serde_json::to_string_pretty(&self.states) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("[Quota] serialize error: {}", e);
                return;
            }
        };
        let tmp = self.cache_path.with_extension("tmp");
        if let Err(e) = self.atomic_write(&tmp, json.as_bytes()) {
            eprintln!("[Quota] failed to persist: {}", e);
        }
    }

    fn atomic_write(&self, tmp: &PathBuf, data: &[u8]) -> Result<(), String> {
        use std::io::Write;
        if let Some(parent) = tmp.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
        let mut f = std::fs::File::create(tmp).map_err(|e| format!("create tmp: {e}"))?;
        f.write_all(data).map_err(|e| format!("write: {e}"))?;
        f.sync_all().map_err(|e| format!("fsync: {e}"))?;
        std::fs::rename(tmp, &self.cache_path).map_err(|e| format!("rename: {e}"))?;
        Ok(())
    }

    pub fn check_availability(&mut self, id: &ProviderId) -> Availability {
        self.reset_if_needed(id);
        let state = match self.states.get_mut(id) {
            Some(s) => s,
            None => return Availability::Exhausted,
        };
        state.refresh_minute_window();
        let limits = ProviderLimits::for_provider(id);

        if state.is_exhausted { return Availability::Exhausted; }
        if state.is_in_cooldown() { return Availability::Exhausted; }

        if let Some(limit) = limits.daily_requests {
            if state.requests_today >= limit {
                state.is_exhausted = true;
                return Availability::Exhausted;
            }
        }
        if let Some(limit) = limits.daily_tokens {
            if state.tokens_today >= limit {
                state.is_exhausted = true;
                return Availability::Exhausted;
            }
        }
        if state.requests_this_min >= limits.rpm {
            let reset_in = (state.min_window_start + 60).saturating_sub(now_unix()).max(1);
            return Availability::RateLimit { wait_secs: reset_in };
        }
        Availability::Ready
    }

    pub fn pick_available(&mut self, candidates: &[ProviderId]) -> PickResult {
        let mut rate_limited: Vec<(ProviderId, u64)> = Vec::new();
        for id in candidates {
            match self.check_availability(id) {
                Availability::Ready => return PickResult::Selected(id.clone()),
                Availability::RateLimit { wait_secs } => {
                    rate_limited.push((id.clone(), wait_secs));
                }
                Availability::Exhausted => {}
            }
        }
        if rate_limited.is_empty() { return PickResult::AllExhausted; }
        let min_wait = rate_limited.iter().map(|(_, w)| *w).min().unwrap_or(u64::MAX);
        if min_wait <= 90 {
            PickResult::WaitFor { seconds: min_wait }
        } else {
            PickResult::AllExhausted
        }
    }

    pub fn record_success(&mut self, id: &ProviderId, tokens_used: u64, latency_ms: f64, kind: &str, repairs: u32) {
        if let Some(state) = self.states.get_mut(id) {
            state.requests_today    += 1;
            state.requests_this_min += 1;
            state.tokens_today      += tokens_used;
            state.consecutive_errors = 0;
            state.cooldown_until     = None;
            state.update_latency(latency_ms);
            state.stats_by_kind.entry(kind.to_string()).or_default().update(true, repairs);
            self.save();
        }
    }

    pub fn record_failure(&mut self, id: &ProviderId, kind: &str, error_kind: &LlmErrorKind) {
        if let Some(state) = self.states.get_mut(id) {
            state.requests_today    += 1;
            state.requests_this_min += 1;
            // CRITICAL BUG FIX: Do not trigger network circuit breaker for Rate Limits!
            if !matches!(error_kind, LlmErrorKind::RateLimit | LlmErrorKind::DailyExhausted | LlmErrorKind::ContextTooLong) {
                state.consecutive_errors += 1;
            }

            if matches!(error_kind, LlmErrorKind::JsonParse) {
                state.json_parse_errors += 1;
            }
            if matches!(error_kind, LlmErrorKind::DailyExhausted) {
                state.is_exhausted = true;
            }
            // Circuit breaker: 5 consecutive technical errors = 15 min cooldown
            if state.consecutive_errors >= 5 {
                state.cooldown_until = Some(now_unix() + 900);
            }
            state.stats_by_kind.entry(kind.to_string()).or_default().update(false, 0);
            self.save();
        }
    }

    pub fn get_state(&self, id: &ProviderId) -> Option<&ProviderState> {
        self.states.get(id)
    }

    fn reset_if_needed(&mut self, id: &ProviderId) {
        if let Some(state) = self.states.get_mut(id) {
            if state.should_reset_daily() {
                state.reset_daily();
            }
        }
    }

    fn default_states() -> HashMap<ProviderId, ProviderState> {
        ProviderId::all().into_iter().map(|id| {
            let state = ProviderState::new(id.clone());
            (id, state)
        }).collect()
    }
}

// ── Status Display ───────────────────────────────────────────

pub fn print_quota_status(mgr: &QuotaManager) {
    println!("\n{}", "=".repeat(80));
    println!("  SPO v2.1 — Provider Quota Status");
    println!("{}", "=".repeat(80));
    println!("{:<12} {:<10} {:<12} {:<8} {:<14} {:<10}",
        "Provider", "Req/Day", "Tok/Day", "Lat(ms)", "Best For", "Status");
    println!("{}", "-".repeat(80));

    for id in ProviderId::display_order() {
        let limits = ProviderLimits::for_provider(id);
        let (req_d, tok_d, lat, status) = match mgr.get_state(id) {
            Some(state) => {
                let rd = match limits.daily_requests {
                    None    => format!("{}/inf", state.requests_today),
                    Some(l) => format!("{}/{}", state.requests_today, l),
                };
                let td = match limits.daily_tokens {
                    None    => "-/inf".to_string(),
                    Some(l) => format!("{}K/{}K", state.tokens_today / 1000, l / 1000),
                };
                let st = if state.is_exhausted { "DONE" }
                    else if state.is_in_cooldown() { "COOLDOWN" }
                    else if state.consecutive_errors >= 3 { "WARN" }
                    else { "READY" };
                (rd, td, state.avg_latency_ms as u32, st)
            }
            None => ("-".into(), "-".into(), 0, "N/A"),
        };
        let best = match id {
            ProviderId::Cerebras   => "All/Fast",
            ProviderId::Mistral    => "Code/Bug",
            ProviderId::Groq       => "TDD/Go",
            ProviderId::OpenRouter => "TS/Variety",
            ProviderId::SambaNova  => "Reasoning",
            ProviderId::Gemini     => "TS/Scaffold",
            ProviderId::GitHub     => "Emergency",
        };
        println!("{:<12} {:<10} {:<12} {:>5}    {:<14} {:<10}",
            id.as_str(), req_d, tok_d, lat, best, status);
    }
    println!("{}\n", "=".repeat(80));
}

// ── Helpers ──────────────────────────────────────────────────

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn quota_cache_path() -> PathBuf {
    let mut p = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("sel-agent");
    std::fs::create_dir_all(&p).ok();
    p.push("quota_state.json");
    p
}

// ── classify_llm_error ───────────────────────────────────────

pub fn classify_llm_error(e: &str) -> LlmErrorKind {
    let msg = e.to_lowercase();
    
    if msg.contains("401") || msg.contains("unauthorized") || msg.contains("invalid api key") || msg.contains("authentication failed") {
        LlmErrorKind::AuthError
    } else if msg.contains("429") || msg.contains("rate limit") || msg.contains("too many") {
        LlmErrorKind::RateLimit
    } else if msg.contains("402") || msg.contains("quota") || msg.contains("daily limit") || msg.contains("exhausted") || msg.contains("insufficient_quota") {
        LlmErrorKind::DailyExhausted
    } else if msg.contains("context") || msg.contains("token limit") || msg.contains("maximum context") || msg.contains("8192") {
        LlmErrorKind::ContextTooLong
    } else if msg.contains("error decoding response body") || msg.contains("expected value") || msg.contains("eof") {
        LlmErrorKind::JsonParse
    } else if msg.contains("network") || msg.contains("connection") || msg.contains("timeout") || msg.contains("503") || msg.contains("502") {
        LlmErrorKind::NetworkError
    } else {
        LlmErrorKind::Unknown
    }
}

#[cfg(test)]
mod quota_tests {
    use super::*;

    #[test]
    fn test_new_provider_defaults() {
        let state = ProviderState::new(ProviderId::Gemini);
        assert_eq!(state.requests_today, 0);
        assert!(!state.is_exhausted);
        assert!(state.reset_at > 0);
    }

    #[test]
    fn test_gemini_daily_limit() {
        let limits = ProviderLimits::for_provider(&ProviderId::Gemini);
        assert_eq!(limits.daily_requests, Some(1_000));
    }

    #[test]
    fn test_groq_limits() {
        let limits = ProviderLimits::for_provider(&ProviderId::Groq);
        assert_eq!(limits.daily_requests, Some(1_000));
        assert_eq!(limits.daily_tokens, None);
    }

    #[test]
    fn test_cerebras_token_only() {
        let limits = ProviderLimits::for_provider(&ProviderId::Cerebras);
        assert_eq!(limits.daily_requests, None);
        assert_eq!(limits.daily_tokens, Some(1_000_000));
    }

    #[test]
    fn test_ema_update() {
        let mut s = TaskKindStats::default();
        s.update(true, 1);
        assert!((s.ema_success - 1.0).abs() < 0.01);
        s.update(false, 0);
        assert!(s.ema_success < 1.0);
        assert!(s.ema_success > 0.5);
    }

    #[test]
    fn test_cooldown_circuit_breaker() {
        let mut state = ProviderState::new(ProviderId::Groq);
        state.consecutive_errors = 4;
        assert!(!state.is_in_cooldown());
        state.cooldown_until = Some(now_unix() + 900);
        assert!(state.is_in_cooldown());
    }

    #[test]
    fn test_classify_errors() {
        assert!(matches!(classify_llm_error("429 Too Many Requests"), LlmErrorKind::RateLimit));
        assert!(matches!(classify_llm_error("daily limit exceeded"), LlmErrorKind::DailyExhausted));
        assert!(matches!(classify_llm_error("maximum context length"), LlmErrorKind::ContextTooLong));
    }
}
