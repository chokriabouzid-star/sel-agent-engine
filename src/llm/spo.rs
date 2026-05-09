// src/llm/spo.rs — Smart Provider Orchestra v2.1
// Task-aware provider selection with self-calibration

use super::quota::{ProviderId, QuotaManager, PickResult, LlmErrorKind, classify_llm_error};

// ── Task Classification ──────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskKind {
    TDD,
    BugFix,
    Scaffold,
    Algorithm,
    Refactor,
}

impl TaskKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TDD       => "TDD",
            Self::BugFix    => "BugFix",
            Self::Scaffold  => "Scaffold",
            Self::Algorithm => "Algorithm",
            Self::Refactor  => "Refactor",
        }
    }
}

/// Classify task from goal text — no LLM needed
pub fn classify_task(goal: &str, _language: &str) -> TaskKind {
    let g = goal.to_lowercase();

    const BUGFIX: &[&str] = &[
        "fix", "bug", "wrong", "broken", "error", "failing",
        "incorrect", "doesn't work", "doesn't pass", "repair",
        "correct", "wrong output",
    ];
    if BUGFIX.iter().any(|w| g.contains(w)) { return TaskKind::BugFix; }

    const REFACTOR: &[&str] = &[
        "refactor", "restructure", "reorganize", "clean up",
        "simplify", "extract", "rename", "move",
    ];
    if REFACTOR.iter().any(|w| g.contains(w)) { return TaskKind::Refactor; }

    const SCAFFOLD: &[&str] = &[
        "create app", "scaffold", "setup project", "new project",
        "initialize", "boilerplate", "starter", "template",
        "set up", "bootstrap",
    ];
    if SCAFFOLD.iter().any(|w| g.contains(w)) { return TaskKind::Scaffold; }

    const ALGO: &[&str] = &[
        "algorithm", "parser", "calculator", "solver",
        "sort", "search", "compute", "calculate",
        "implement function", "data structure",
    ];
    if ALGO.iter().any(|w| g.contains(w)) { return TaskKind::Algorithm; }

    TaskKind::TDD
}

// ── Specialization Matrix ────────────────────────────────────

/// Build ordered candidate list by (TaskKind, Language)
pub fn get_ordered_candidates(kind: &TaskKind, language: &str) -> Vec<ProviderId> {
    use ProviderId::*;
    let lang = language.to_lowercase();

    match (kind, lang.as_str()) {
        // Rust — deep thinking needed
        (TaskKind::BugFix | TaskKind::Algorithm, "rust") => vec![
            Cerebras, Groq, OpenRouter, Mistral, SambaNova, Gemini, GitHub,
        ],
        (TaskKind::TDD | TaskKind::Scaffold | TaskKind::Refactor, "rust") => vec![
            Cerebras, Groq, Mistral, OpenRouter, GitHub,
        ],

        // TypeScript
        (TaskKind::Scaffold | TaskKind::TDD, "typescript" | "ts") => vec![
            Cerebras, Gemini, Groq, OpenRouter, Mistral, GitHub,
        ],

        // Go — simple syntax, speed matters
        (_, "go" | "golang") => vec![
            Groq, SambaNova, Cerebras, Gemini, GitHub,
        ],

        // Python TDD — speed + iteration
        (TaskKind::TDD, "python" | "py") => vec![
            Groq, Cerebras, SambaNova, OpenRouter, Mistral, GitHub,
        ],

        // Python BugFix/Algorithm — deep thinking
        (TaskKind::BugFix | TaskKind::Algorithm, "python" | "py") => vec![
            Cerebras, Groq, SambaNova, OpenRouter, Mistral, Gemini, GitHub,
        ],

        // Node.js / JavaScript
        (TaskKind::Scaffold, "node" | "javascript" | "js") => vec![
            Cerebras, Gemini, Groq, OpenRouter, GitHub,
        ],
        (TaskKind::TDD | TaskKind::BugFix, "node" | "javascript" | "js") => vec![
            Groq, Cerebras, OpenRouter, Gemini, GitHub,
        ],

        // Default
        _ => vec![
            Cerebras, Groq, Gemini, OpenRouter, SambaNova, Mistral, GitHub,
        ],
    }
}

// ── Self-Calibration ─────────────────────────────────────────

/// Adjust ordering based on actual performance data.
/// Constraints: min 5 samples, one swap per call, GitHub always last.
pub fn recalibrate_if_needed(
    candidates: &mut Vec<ProviderId>,
    quota_mgr: &QuotaManager,
    kind: &TaskKind,
) {
    let kind_str = kind.as_str();

    let mut scores: Vec<(usize, f64)> = candidates.iter().enumerate().map(|(i, id)| {
        let score = match quota_mgr.get_state(id) {
            None => 0.5,
            Some(state) => match state.stats_by_kind.get(kind_str) {
                None => 0.5,
                Some(s) if !s.is_statistically_valid() => 0.5,
                Some(s) => {
                    let penalty = (s.avg_repairs() - 1.0).max(0.0) * 0.1;
                    (s.ema_success - penalty).clamp(0.0, 1.0)
                }
            },
        };
        (i, score)
    }).collect();

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let desired: Vec<ProviderId> = scores.iter().map(|(i, _)| candidates[*i].clone()).collect();

    // One swap only — gradual adjustment
    for i in 0..desired.len().saturating_sub(1) {
        if desired[i] != candidates[i] {
            if let Some(j) = candidates.iter().position(|x| *x == desired[i]) {
                candidates.swap(i, j);
                break;
            }
        }
    }

    // GitHub always last
    let last = candidates.len() - 1;
    if let Some(pos) = candidates.iter().position(|x| *x == ProviderId::GitHub) {
        if pos != last { candidates.swap(pos, last); }
    }
}

#[cfg(test)]
mod spo_tests {
    use super::*;

    #[test]
    fn test_classify_bugfix() {
        assert_eq!(classify_task("fix the wrong output", "rust"), TaskKind::BugFix);
        assert_eq!(classify_task("the test is failing", "python"), TaskKind::BugFix);
    }

    #[test]
    fn test_classify_scaffold() {
        assert_eq!(classify_task("create app with express", "node"), TaskKind::Scaffold);
    }

    #[test]
    fn test_classify_algorithm() {
        assert_eq!(classify_task("implement sorting algorithm", "python"), TaskKind::Algorithm);
    }

    #[test]
    fn test_classify_refactor() {
        assert_eq!(classify_task("refactor the module", "rust"), TaskKind::Refactor);
    }

    #[test]
    fn test_classify_default_tdd() {
        assert_eq!(classify_task("write function that returns sum", "python"), TaskKind::TDD);
    }

    #[test]
    fn test_rust_bugfix_prefers_cerebras() {
        let c = get_ordered_candidates(&TaskKind::BugFix, "rust");
        assert_eq!(c[0], ProviderId::Cerebras);
    }

    #[test]
    fn test_go_prefers_groq() {
        let c = get_ordered_candidates(&TaskKind::TDD, "go");
        assert_eq!(c[0], ProviderId::Groq);
    }

    #[test]
    fn test_github_always_last() {
        for kind in &[TaskKind::TDD, TaskKind::BugFix, TaskKind::Scaffold, TaskKind::Algorithm] {
            for lang in &["rust", "python", "typescript", "go"] {
                let c = get_ordered_candidates(kind, lang);
                assert_eq!(c.last(), Some(&ProviderId::GitHub),
                    "GitHub should be last for {:?}/{}", kind, lang);
            }
        }
    }
}


// ── SmartProviderOrchestra ───────────────────────────────────

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::Duration;
use super::pattern_memory::{PatternMemory, KnownSolution};

use crate::llm::{LLMRequest, LLMResponse, live::call_provider_api};

pub struct SmartProviderOrchestra {
    quota_mgr:  Arc<Mutex<QuotaManager>>,
    memory:     Arc<Mutex<PatternMemory>>,
    call_count: AtomicU32,
}

impl SmartProviderOrchestra {
    pub fn new() -> Self {
        Self {
            quota_mgr:  Arc::new(Mutex::new(QuotaManager::load())),
            memory:     Arc::new(Mutex::new(PatternMemory::load())),
            call_count: AtomicU32::new(0),
        }
    }

    pub async fn select_and_call(
        &self,
        request: &LLMRequest,
        goal_text: &str,
        language: &str,
        error_context: Option<&str>,
        repairs_needed: u32,
    ) -> Result<(LLMResponse, String, String), SpoError> {
        let kind = classify_task(goal_text, language);
        
        let mut candidates = get_ordered_candidates(&kind, language);

        let count = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;
        if count % 10 == 0 {
            let mgr = self.quota_mgr.lock().await;
            recalibrate_if_needed(&mut candidates, &mgr, &kind);
        }

        let memory_hint = if let Some(err) = error_context {
            let mem = self.memory.lock().await;
            mem.lookup(err, language)
        } else {
            None
        };

        self.call_with_retry(request, &kind, language, error_context, repairs_needed, memory_hint, &candidates).await
    }

    async fn call_with_retry(
        &self,
        request:        &LLMRequest,
        kind:           &TaskKind,
        language:       &str,
        error_context:  Option<&str>,
        repairs_needed: u32,
        hint:           Option<KnownSolution>,
        fallback_order: &[ProviderId],
    ) -> Result<(LLMResponse, String, String), SpoError> {
        let mut tried: Vec<ProviderId> = Vec::new();
        let mut json_retries = 0u32;

        let mut current = match {
            let mut mgr = self.quota_mgr.lock().await;
            mgr.pick_available(fallback_order)
        } {
            PickResult::Selected(p) => p,
            PickResult::WaitFor { seconds } => {
                self.wait_and_log(seconds).await;
                let mut mgr = self.quota_mgr.lock().await;
                match mgr.pick_available(fallback_order) {
                    PickResult::Selected(p) => p,
                    _ => return Err(SpoError::NoProviderAvailable),
                }
            }
            PickResult::AllExhausted => return Err(SpoError::NoProviderAvailable),
        };

        let mut spo_tracing = false;
        loop {
            if !tried.contains(&current) {
                tried.push(current.clone());
            }

            let enriched_req = enrich_with_hint(request, &hint);

            let start = std::time::Instant::now();
            let result = call_provider_api(&current, &enriched_req).await;
            let elapsed_ms = start.elapsed().as_millis() as f64;

            match result {
                Ok(response) => {
                    if spo_tracing {
                        eprintln!("{} ✓", current.as_str());
                    }
                    {
                        let mut mgr = self.quota_mgr.lock().await;
                        let tokens = (response.tokens_in + response.tokens_out) as u64;
                        mgr.record_success(&current, tokens, elapsed_ms, kind.as_str(), repairs_needed);
                        crate::cost::record_session_usage(current.as_str(), response.tokens_in, response.tokens_out);
                    }

                    if repairs_needed > 0 {
                        if let Some(err_ctx) = error_context {
                            let mut mem = self.memory.lock().await;
                            mem.record(
                                language,
                                err_ctx,
                                &extract_solution_hint(&response.content),
                                current.as_str(),
                                repairs_needed,
                            );
                        }
                    }

                    return Ok((response, current.as_str().to_string(), kind.as_str().to_string()));
                }
                Err(e) => {
                    let err_kind = classify_llm_error(&e.to_string());

                    if matches!(err_kind, LlmErrorKind::JsonParse) && json_retries <= 1 {
                        // Don't trace normal json retries inline unless it exceeds
                    } else if !spo_tracing {
                        eprint!("   [SPO] ");
                        spo_tracing = true;
                    }

                    match err_kind {
                        LlmErrorKind::JsonParse => {
                            json_retries += 1;
                            if json_retries <= 2 {
                                eprintln!("\n[SPO] JSON parse error on {}: retry {}/2", current.as_str(), json_retries);
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                continue;
                            } else {
                                eprint!("{} ⨯ (JSON) ➔ ", current.as_str());
                            }
                        }
                        LlmErrorKind::ContextTooLong => eprint!("{} ⨯ (TooLong) ➔ ", current.as_str()),
                        LlmErrorKind::RateLimit => eprint!("{} ⨯ (429) ➔ ", current.as_str()),
                        LlmErrorKind::DailyExhausted => eprint!("{} ⨯ (Exhausted) ➔ ", current.as_str()),
                        LlmErrorKind::AuthError => eprint!("{} ⨯ (401) ➔ ", current.as_str()),
                        LlmErrorKind::NetworkError | LlmErrorKind::Unknown => eprint!("{} ⨯ (Err) ➔ ", current.as_str()),
                    }

                    {
                        let mut mgr = self.quota_mgr.lock().await;
                        mgr.record_failure(&current, kind.as_str(), &err_kind);
                    }

                    let remaining: Vec<ProviderId> = fallback_order.iter()
                        .filter(|p| !tried.contains(p))
                        .cloned()
                        .collect();

                    json_retries = 0;

                    let next_pick = {
                        let mut mgr = self.quota_mgr.lock().await;
                        mgr.pick_available(&remaining)
                    };

                    match next_pick {
                        PickResult::Selected(next) => {
                            current = next;
                        }
                        PickResult::WaitFor { seconds } if seconds <= 90 => {
                            self.wait_and_log(seconds).await;
                            let next_pick_after = {
                                let mut mgr = self.quota_mgr.lock().await;
                                mgr.pick_available(&remaining)
                            };
                            match next_pick_after {
                                PickResult::Selected(next) => { current = next; }
                                _ => {
                                    let mgr = self.quota_mgr.lock().await;
                                    eprintln!("{}", format_exhaustion_report(&mgr));
                                    return Err(SpoError::NoProviderAvailable);
                                }
                            }
                        }
                        _ => {
                            let mgr = self.quota_mgr.lock().await;
                            eprintln!("{}", format_exhaustion_report(&mgr));
                            return Err(SpoError::NoProviderAvailable);
                        }
                    }
                }
            }
        }
    }

    async fn wait_and_log(&self, seconds: u64) {
        let mut remaining = seconds;
        while remaining > 0 {
            eprint!("\r[SPO] ⏳ resuming in {}s...   ", remaining);
            tokio::time::sleep(Duration::from_secs(5.min(remaining))).await;
            remaining = remaining.saturating_sub(5);
        }
        eprintln!("\r[SPO] ✓ ready — resuming         ");
    }
}

pub fn format_exhaustion_report(quota: &crate::llm::quota::QuotaManager) -> String {
    let mut lines = vec![
        "\n❌ All providers exhausted:".to_string()
    ];
    let now = crate::llm::quota::now_unix();
    for id in crate::llm::quota::ProviderId::all() {
        if let Some(state) = quota.get_state(&id) {
            let status = if state.is_exhausted {
                "Daily Limit Hit".to_string()
            } else if let Some(cooldown) = state.cooldown_until {
                if cooldown > now {
                    format!("Cooldown for {}s", cooldown - now)
                } else {
                    "Ready".to_string()
                }
            } else {
                format!("Calls: {}", state.requests_today)
            };
            lines.push(format!("   {:<12} → {}", id.as_str(), status));
        }
    }
    lines.push("\n💡 Run again later or check your API keys with:".to_string());
    lines.push("   sel-agent quota --status\n".to_string());
    lines.join("\n")
}

pub fn enrich_with_hint(request: &LLMRequest, hint: &Option<KnownSolution>) -> LLMRequest {
    if let Some(h) = hint {
        let mut cloned = request.clone();
        cloned.system = format!("{}\n\n[SPO PATTERN MEMORY]: {}", cloned.system, h.hint);
        cloned
    } else {
        request.clone()
    }
}

pub fn extract_solution_hint(content: &str) -> String {
    content.chars().take(120).collect()
}

#[derive(Debug)]
pub enum SpoError {
    NoProviderAvailable,
    MaxRetriesExceeded,
}

impl std::fmt::Display for SpoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoProviderAvailable => write!(f, "All LLM providers exhausted — try again in 24h"),
            Self::MaxRetriesExceeded => write!(f, "Max retries exceeded for all providers"),
        }
    }
}
