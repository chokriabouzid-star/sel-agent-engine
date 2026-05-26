use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct LimitTracker {
    daily_dead: HashSet<String>,   //    
    rpm_dead_until: std::collections::HashMap<String, u64>, //  X 
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
        }
    }

    pub fn mark_daily(&mut self, provider: &str) {
        eprintln!(" [{}] daily limit hit  skipping for rest of session", provider);
        self.daily_dead.insert(provider.to_string());
    }

    pub fn mark_rpm(&mut self, provider: &str, wait_secs: u64) {
        let resume_at = self.now() + wait_secs;
        self.rpm_dead_until.insert(provider.to_string(), resume_at);
        eprintln!(" [{}] RPM limit  cooling {}s", provider, wait_secs);
    }

    pub fn is_available(&self, provider: &str) -> bool {
        if self.daily_dead.contains(provider) {
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
            .unwrap()
            .as_secs()
    }
}
