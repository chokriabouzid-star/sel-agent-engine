//! Smart Provider Orchestra (SPO)  multi-LLM provider management.
//!
//! Implements intelligent task-based provider selection, atomic quota tracking,
//! pattern-memory error recovery, and graceful fallback chains.
//!
//! # Provider Priority (default)
//! 1. Cerebras   ultra-fast, ideal for TypeScript/Go scaffolding
//! 2. Groq       high-throughput, good for Rust
//! 3. SambaNova  cost-effective, no streaming required
//! 4. Gemini     broad capability, verbose output
//! 5. OpenAI     fallback of last resort

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Provider identifiers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    Cerebras,
    Groq,
    SambaNova,
    Gemini,
    OpenAI,
}

impl ProviderId {
    pub fn name(&self) -> &'static str {
        match self {
            ProviderId::Cerebras => "Cerebras",
            ProviderId::Groq => "Groq",
            ProviderId::SambaNova => "SambaNova",
            ProviderId::Gemini => "Gemini",
            ProviderId::OpenAI => "OpenAI",
        }
    }

    /// Default ordered list of all providers, highest priority first.
    pub fn all_ordered() -> &'static [ProviderId] {
        &[
            ProviderId::Cerebras,
            ProviderId::Groq,
            ProviderId::SambaNova,
            ProviderId::Gemini,
            ProviderId::OpenAI,
        ]
    }
}

// ---------------------------------------------------------------------------
// Task classification
// ---------------------------------------------------------------------------

/// Task category used to select the best-suited provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// Fast scaffolding / boilerplate generation
    Scaffolding,
    /// Complex reasoning, architecture decisions
    Reasoning,
    /// Straightforward bug repair
    Repair,
    /// Test generation
    Testing,
}

impl TaskKind {
    /// Classify a task from its description text.
    pub fn classify(goal: &str) -> Self {
        let goal_lower = goal.to_lowercase();

        if goal_lower.contains("scaffold")
            || goal_lower.contains("boilerplate")
            || goal_lower.contains("create project")
            || goal_lower.contains("init")
        {
            return TaskKind::Scaffolding;
        }
        if goal_lower.contains("architect")
            || goal_lower.contains("design")
            || goal_lower.contains("refactor")
            || goal_lower.contains("explain")
        {
            return TaskKind::Reasoning;
        }
        if goal_lower.contains("test")
            || goal_lower.contains("spec")
            || goal_lower.contains("assert")
        {
            return TaskKind::Testing;
        }
        // Default: treat as repair
        TaskKind::Repair
    }

    /// Return the preferred provider order for this task kind.
    pub fn preferred_providers(&self) -> Vec<ProviderId> {
        match self {
            TaskKind::Scaffolding => vec![
                ProviderId::Cerebras,
                ProviderId::Groq,
                ProviderId::SambaNova,
                ProviderId::Gemini,
                ProviderId::OpenAI,
            ],
            TaskKind::Reasoning => vec![
                ProviderId::Gemini,
                ProviderId::OpenAI,
                ProviderId::Groq,
                ProviderId::Cerebras,
                ProviderId::SambaNova,
            ],
            TaskKind::Testing => vec![
                ProviderId::Groq,
                ProviderId::Cerebras,
                ProviderId::Gemini,
                ProviderId::OpenAI,
                ProviderId::SambaNova,
            ],
            TaskKind::Repair => vec![
                ProviderId::Groq,
                ProviderId::Cerebras,
                ProviderId::SambaNova,
                ProviderId::Gemini,
                ProviderId::OpenAI,
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Quota management
// ---------------------------------------------------------------------------

/// Per-provider quota state, persisted atomically in memory.
#[derive(Debug, Clone)]
pub struct ProviderQuota {
    pub requests_used: u32,
    pub requests_limit: u32,
    pub cooldown_until: Option<Instant>,
    pub consecutive_errors: u32,
}

impl ProviderQuota {
    pub fn new(limit: u32) -> Self {
        Self {
            requests_used: 0,
            requests_limit: limit,
            cooldown_until: None,
            consecutive_errors: 0,
        }
    }

    pub fn is_available(&self) -> bool {
        if self.requests_used >= self.requests_limit {
            return false;
        }
        if let Some(until) = self.cooldown_until {
            if Instant::now() < until {
                return false;
            }
        }
        self.consecutive_errors < 5
    }

    pub fn record_success(&mut self) {
        self.requests_used += 1;
        self.consecutive_errors = 0;
    }

    pub fn record_error(&mut self, cooldown_secs: u64) {
        self.consecutive_errors += 1;
        if self.consecutive_errors >= 3 {
            self.cooldown_until = Some(Instant::now() + Duration::from_secs(cooldown_secs));
            warn!(
                consecutive_errors = self.consecutive_errors,
                cooldown_secs, "provider entering cooldown"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Pattern memory  error recovery cache
// ---------------------------------------------------------------------------

/// Associates error fingerprints with providers that previously solved them.
#[derive(Debug, Default)]
pub struct PatternMemory {
    /// error_fingerprint  provider that resolved it
    cache: HashMap<String, ProviderId>,
}

impl PatternMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `provider` successfully resolved an error matching `error`.
    pub fn record(&mut self, error: &str, provider: ProviderId) {
        let key = fingerprint(error);
        self.cache.insert(key, provider);
    }

    /// Look up which provider previously solved a similar error.
    pub fn lookup(&self, error: &str) -> Option<ProviderId> {
        let key = fingerprint(error);
        self.cache.get(&key).copied()
    }
}

/// Compute a short fingerprint from an error string (first 80 non-whitespace chars).
fn fingerprint(error: &str) -> String {
    error
        .chars()
        .filter(|c| !c.is_whitespace())
        .take(80)
        .collect()
}

// ---------------------------------------------------------------------------
// Smart Provider Orchestra
// ---------------------------------------------------------------------------

/// The orchestrator  selects the best provider for each request and handles fallback.
pub struct SmartProviderOrchestra {
    quotas: Arc<Mutex<HashMap<ProviderId, ProviderQuota>>>,
    memory: Arc<Mutex<PatternMemory>>,
    /// Provider currently locked-in for the session ("sticky" provider)
    sticky: Arc<Mutex<Option<ProviderId>>>,
}

impl SmartProviderOrchestra {
    /// Create a new orchestra with default quotas.
    pub fn new() -> Self {
        let mut quotas = HashMap::new();
        quotas.insert(ProviderId::Cerebras, ProviderQuota::new(1000));
        quotas.insert(ProviderId::Groq, ProviderQuota::new(500));
        quotas.insert(ProviderId::SambaNova, ProviderQuota::new(300));
        quotas.insert(ProviderId::Gemini, ProviderQuota::new(200));
        quotas.insert(ProviderId::OpenAI, ProviderQuota::new(100));

        Self {
            quotas: Arc::new(Mutex::new(quotas)),
            memory: Arc::new(Mutex::new(PatternMemory::new())),
            sticky: Arc::new(Mutex::new(None)),
        }
    }

    /// Select the best available provider for the given task and optional previous error.
    pub fn select(&self, task: TaskKind, prev_error: Option<&str>) -> Option<ProviderId> {
        let quotas = match self.quotas.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("quotas lock poisoned: {}", e);
                return None;
            }
        };

        // 1. Check sticky provider
        {
            let sticky = match self.sticky.lock() {
                Ok(g) => g,
                Err(e) => {
                    tracing::error!("sticky lock poisoned: {}", e);
                    return None;
                }
            };
            if let Some(p) = *sticky {
                if quotas.get(&p).map(|q| q.is_available()).unwrap_or(false) {
                    debug!(provider = p.name(), "using sticky provider");
                    return Some(p);
                }
            }
        }

        // 2. Check pattern memory for known-good provider for this error type
        if let Some(error) = prev_error {
            let memory = match self.memory.lock() {
                Ok(g) => g,
                Err(e) => {
                    tracing::error!("memory lock poisoned: {}", e);
                    return None;
                }
            };
            if let Some(p) = memory.lookup(error) {
                if quotas.get(&p).map(|q| q.is_available()).unwrap_or(false) {
                    info!(provider = p.name(), "pattern memory hit");
                    return Some(p);
                }
            }
        }

        // 3. Try preferred providers for the task kind
        for p in task.preferred_providers() {
            if quotas.get(&p).map(|q| q.is_available()).unwrap_or(false) {
                info!(provider = p.name(), task = ?task, "selected provider");
                return Some(p);
            }
        }

        // 4. All providers exhausted
        warn!("all providers unavailable");
        None
    }

    /// Record the result of a provider call.
    pub fn record_outcome(&self, provider: ProviderId, success: bool, error: Option<&str>) {
        let mut quotas = match self.quotas.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("quotas lock poisoned in record_outcome: {}", e);
                return;
            }
        };
        if let Some(q) = quotas.get_mut(&provider) {
            if success {
                q.record_success();
                // Lock in this provider as sticky
                if let Ok(mut sticky) = self.sticky.lock() {
                    *sticky = Some(provider);
                }
                // Record in pattern memory if there was a previous error
                if let Some(err) = error {
                    if let Ok(mut memory) = self.memory.lock() {
                        memory.record(err, provider);
                    }
                }
            } else {
                q.record_error(60);
                // Clear sticky on error
                let mut sticky = match self.sticky.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        tracing::error!("sticky lock poisoned on error: {}", e);
                        return;
                    }
                };
                if *sticky == Some(provider) {
                    *sticky = None;
                }
            }
        }
    }
}

impl Default for SmartProviderOrchestra {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_kind_classify_scaffolding() {
        assert_eq!(
            TaskKind::classify("scaffold a Go project"),
            TaskKind::Scaffolding
        );
    }

    #[test]
    fn test_task_kind_classify_repair() {
        assert_eq!(TaskKind::classify("fix the add function"), TaskKind::Repair);
    }

    #[test]
    fn test_task_kind_classify_testing() {
        assert_eq!(
            TaskKind::classify("write tests for parser"),
            TaskKind::Testing
        );
    }

    #[test]
    fn test_quota_availability() {
        let mut q = ProviderQuota::new(10);
        assert!(q.is_available());
        q.requests_used = 10;
        assert!(!q.is_available());
    }

    #[test]
    fn test_quota_cooldown() {
        let mut q = ProviderQuota::new(100);
        q.record_error(3600);
        q.record_error(3600);
        q.record_error(3600); // 3rd  enters cooldown
        assert!(!q.is_available());
    }

    #[test]
    fn test_pattern_memory_roundtrip() {
        let mut mem = PatternMemory::new();
        mem.record("undefined: fmt", ProviderId::Groq);
        assert_eq!(mem.lookup("undefined: fmt"), Some(ProviderId::Groq));
    }

    #[test]
    fn test_pattern_memory_miss() {
        let mem = PatternMemory::new();
        assert_eq!(mem.lookup("totally new error"), None);
    }

    #[test]
    fn test_spo_selects_provider() {
        let spo = SmartProviderOrchestra::new();
        let p = spo.select(TaskKind::Repair, None);
        assert!(p.is_some());
    }

    #[test]
    fn test_spo_sticky_after_success() {
        let spo = SmartProviderOrchestra::new();
        let p = spo
            .select(TaskKind::Repair, None)
            .expect("test setup/use should succeed");
        spo.record_outcome(p, true, None);
        // Sticky should now be set
        let sticky = *spo.sticky.lock().expect("test setup/use should succeed");
        assert_eq!(sticky, Some(p));
    }
}
