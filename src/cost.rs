use std::sync::{Mutex, OnceLock};
use std::collections::HashMap;

static SESSION_STATS: OnceLock<Mutex<HashMap<String, ProviderUsage>>> = OnceLock::new();

#[derive(Default, Clone)]
pub struct ProviderUsage {
    pub calls: u32,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

pub struct CostTracker {}

impl CostTracker {
    pub fn new() -> Self {
        Self {}
    }

    pub fn add_usage(&self, _t_in: u32, _t_out: u32, _calls: u32) {
        // Obsolete: We now use record_session_usage directly from SPO
    }

    pub fn print_summary(&self, _provider_model: &str) {
        print_session_summary();
    }
}

pub fn record_session_usage(provider: &str, t_in: u32, t_out: u32) {
    let stats_mutex = SESSION_STATS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut stats) = stats_mutex.lock() {
        let usage = stats.entry(provider.to_string()).or_insert_with(ProviderUsage::default);
        usage.calls += 1;
        usage.tokens_in += t_in as u64;
        usage.tokens_out += t_out as u64;
    }
}

pub fn print_session_summary() {
    let stats_mutex = SESSION_STATS.get_or_init(|| Mutex::new(HashMap::new()));
    let stats = stats_mutex.lock().unwrap();
    if stats.is_empty() { return; }

    println!("\n💸 Session Summary:");
    println!("   ┌──────────────┬───────┬──────────┬──────────┐");
    println!("   │ Provider     │ Calls │ Tokens   │ Est.$    │");
    println!("   ├──────────────┼───────┼──────────┼──────────┤");

    let mut total_calls = 0;
    let mut total_tokens = 0;
    let mut total_cost = 0.0;

    let mut sorted_stats: Vec<_> = stats.iter().collect();
    sorted_stats.sort_by(|a, b| a.0.cmp(b.0));

    for (p, u) in sorted_stats {
        let tokens = u.tokens_in + u.tokens_out;
        let cost = estimate_usd(p, u.tokens_in, u.tokens_out);
        println!("   │ {:<12} │ {:>5} │ {:>8} │ ${:<8.3} │", truncate(p, 12), u.calls, tokens, cost);
        total_calls += u.calls;
        total_tokens += tokens;
        total_cost += cost;
    }
    println!("   ├──────────────┼───────┼──────────┼──────────┤");
    println!("   │ {:<12} │ {:>5} │ {:>8} │ ${:<8.3} │", "Total", total_calls, total_tokens, total_cost);
    println!("   └──────────────┴───────┴──────────┴──────────┘");
}

fn truncate(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        None => s.to_string(),
        Some((idx, _)) => s[..idx].to_string(),
    }
}

fn estimate_usd(provider: &str, in_tokens: u64, out_tokens: u64) -> f64 {
    let p = provider.to_lowercase();
    if p.contains("cerebras") || p.contains("groq") || p.contains("sambanova") || p.contains("gemini") {
        return 0.0;
    }
    let in_k = in_tokens as f64 / 1000.0;
    let out_k = out_tokens as f64 / 1000.0;
    in_k * 0.001 + out_k * 0.002
}
