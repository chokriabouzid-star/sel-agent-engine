use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const EXHAUSTION_TTL_SECS: u64 = 86400; // 24 hours

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct KeyState {
    pub exhausted_at: Option<u64>, // Unix timestamp
    pub expired: bool,             // v7.9.9 P2b: permanently expired
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ProviderState {
    pub keys: Vec<KeyState>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct ProviderStateCache {
    pub providers: HashMap<String, ProviderState>,
}

impl ProviderStateCache {
    fn state_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".sel-agent")
            .join("provider_state.json")
    }

    pub fn load() -> Self {
        let path = Self::state_path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::state_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }

    pub fn mark_exhausted(&mut self, provider: &str, key_idx: usize) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let entry = self.providers.entry(provider.to_string()).or_default();

        // Expand array if needed
        while entry.keys.len() <= key_idx {
            entry.keys.push(KeyState::default());
        }

        entry.keys[key_idx].exhausted_at = Some(now);
        self.save(); // Save immediately
    }

    pub fn mark_permanently_expired(&mut self, provider: &str, key_idx: usize) {
        let entry = self.providers.entry(provider.to_string()).or_default();

        while entry.keys.len() <= key_idx {
            entry.keys.push(KeyState::default());
        }

        entry.keys[key_idx].expired = true;
        self.save();
    }

    pub fn is_key_exhausted(&self, provider: &str, key_idx: usize) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        match self
            .providers
            .get(provider)
            .and_then(|p| p.keys.get(key_idx))
        {
            None => false,
            Some(k) => {
                if k.expired {
                    return true;
                }
                if let Some(t) = k.exhausted_at {
                    if now.saturating_sub(t) < EXHAUSTION_TTL_SECS {
                        return true;
                    }
                }
                false
            }
        }
    }

    pub fn available_key_count(&self, provider: &str, total_keys: usize) -> usize {
        (0..total_keys)
            .filter(|&i| !self.is_key_exhausted(provider, i))
            .count()
    }

    pub fn estimated_remaining_calls(&self, providers: &[(&str, usize)]) -> usize {
        providers
            .iter()
            .map(|(name, key_count)| self.available_key_count(name, *key_count) * 50)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_and_detect_exhausted() {
        let mut cache = ProviderStateCache::default();

        assert!(!cache.is_key_exhausted("groq", 0));

        cache.mark_exhausted("groq", 0);
        assert!(cache.is_key_exhausted("groq", 0));
    }

    #[test]
    fn test_available_count() {
        let mut cache = ProviderStateCache::default();
        cache.mark_exhausted("gemini", 0);
        cache.mark_exhausted("gemini", 1);
        assert_eq!(cache.available_key_count("gemini", 4), 2);
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut cache = ProviderStateCache::default();
        cache.mark_exhausted("groq", 0);
        cache.mark_exhausted("cerebras", 2);

        let json = serde_json::to_string(&cache).expect("serialize ProviderStateCache in test");
        let loaded: ProviderStateCache =
            serde_json::from_str(&json).expect("deserialize ProviderStateCache in test");

        assert!(loaded.is_key_exhausted("groq", 0));
        assert!(loaded.is_key_exhausted("cerebras", 2));
        assert!(!loaded.is_key_exhausted("gemini", 0));
    }

    #[test]
    fn test_preflight_with_burned_keys() {
        let mut cache = ProviderStateCache::default();
        for i in 0..4 {
            cache.mark_exhausted("groq", i);
        }
        for i in 0..4 {
            cache.mark_exhausted("gemini", i);
        }
        for i in 0..4 {
            cache.mark_exhausted("cerebras", i);
        }

        let providers = vec![
            ("groq", 4),
            ("gemini", 4),
            ("cerebras", 4),
            ("openrouter", 1),
            ("github", 1),
        ];

        let remaining = cache.estimated_remaining_calls(&providers);
        assert_eq!(remaining, 100);
    }
}
