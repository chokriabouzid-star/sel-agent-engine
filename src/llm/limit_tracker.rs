use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct LimitTracker {
    daily_dead: HashSet<String>,
    rpm_dead_until: std::collections::HashMap<String, u64>,
    rejected_this_session: HashSet<String>, // deterministic provider rejections — never persisted to disk, cleared on next process run
}

impl Default for LimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl LimitTracker {
    pub fn new() -> Self {
        Self {
            daily_dead: HashSet::new(),
            rpm_dead_until: std::collections::HashMap::new(),
            rejected_this_session: HashSet::new(),
        }
    }

    pub fn mark_daily(&mut self, provider: &str) {
        eprintln!(
            " [{}] daily limit hit  skipping for rest of session",
            provider
        );
        self.daily_dead.insert(provider.to_string());
    }

    pub fn mark_rpm(&mut self, provider: &str, wait_secs: u64) {
        let resume_at = self.now() + wait_secs;
        self.rpm_dead_until.insert(provider.to_string(), resume_at);
        eprintln!(" [{}] RPM limit  cooling {}s", provider, wait_secs);
    }

    /// Provider rejected the request deterministically (bad request / wrong
    /// model-or-endpoint / ambiguous 401 without explicit key-invalidity
    /// evidence). This is NOT proof of quota exhaustion or a dead key.
    /// Skipped only for the rest of THIS process run — never written to disk,
    /// so the next `sel-agent` invocation retries this provider fresh.
    pub fn mark_rejected(&mut self, provider: &str) {
        eprintln!(
            "   ⚠️  [{}] rejected the request (non-quota) — skipping for rest of this run",
            provider
        );
        self.rejected_this_session.insert(provider.to_string());
    }

    pub fn is_available(&self, provider: &str) -> bool {
        if self.daily_dead.contains(provider) {
            return false;
        }
        if self.rejected_this_session.contains(provider) {
            return false;
        }
        if let Some(&until) = self.rpm_dead_until.get(provider) {
            return self.now() >= until;
        }
        true
    }

    pub fn any_available(&self, providers: &[String]) -> bool {
        providers.iter().any(|p| self.is_available(p))
    }

    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_rejected_makes_provider_unavailable_this_session() {
        let mut t = LimitTracker::new();
        assert!(t.is_available("Gemini"));
        t.mark_rejected("Gemini");
        assert!(!t.is_available("Gemini"));
    }

    #[test]
    fn mark_rejected_does_not_affect_other_providers() {
        let mut t = LimitTracker::new();
        t.mark_rejected("Gemini");
        assert!(t.is_available("Groq"));
        assert!(t.is_available("GitHub"));
    }

    #[test]
    fn mark_rejected_is_independent_of_daily_and_rpm_state() {
        let mut t = LimitTracker::new();
        t.mark_daily("Cerebras");
        t.mark_rejected("Gemini");
        assert!(!t.is_available("Cerebras")); // daily — unaffected by this change
        assert!(!t.is_available("Gemini")); // rejected — new behavior
        assert!(t.is_available("Groq")); // untouched
    }
}
