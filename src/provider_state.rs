use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const EXHAUSTION_TTL_SECS: u64 = 86400; // 24 hours

/// Stable identifier for a provider key. This is for cache identity only,
/// not authentication; the raw key is never written to the state cache.
pub fn key_fingerprint(key: &str) -> String {
    let mut hash: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for byte in key.as_bytes() {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    format!("fnv1a128:{hash:032x}")
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct KeyState {
    pub exhausted_at: Option<u64>, // Unix timestamp
    pub expired: bool,             // v7.9.9 P2b: permanently expired
    #[serde(default)]
    pub key_fp: Option<String>,
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
    pub fn state_path() -> PathBuf {
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
        // v9.3.4 SAFETY: cache MUST NOT write to the real user cache during
        // `cargo test`. Historic bug: `test_preflight_with_burned_keys`
        // burned 12 real provider keys in $HOME/.sel-agent/provider_state.json,
        // paralysing every subsequent live run. Under #[cfg(test)] this
        // becomes a deliberate no-op; in-memory state is preserved and the
        // existing tests (which only assert in-memory behaviour) are unaffected.
        #[cfg(test)]
        {}
        #[cfg(not(test))]
        {
            let path = Self::state_path();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(&path, json);
            }
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

    fn key_state_mut_for(&mut self, provider: &str, key_idx: usize, key_fp: &str) -> &mut KeyState {
        let provider_state = self.providers.entry(provider.to_string()).or_default();

        if let Some(index) = provider_state
            .keys
            .iter()
            .position(|state| state.key_fp.as_deref() == Some(key_fp))
        {
            return &mut provider_state.keys[index];
        }

        if key_idx >= provider_state.keys.len() {
            provider_state
                .keys
                .resize_with(key_idx + 1, KeyState::default);
        }

        if provider_state.keys[key_idx].key_fp.is_none() {
            provider_state.keys[key_idx] = KeyState {
                key_fp: Some(key_fp.to_owned()),
                ..KeyState::default()
            };
            return &mut provider_state.keys[key_idx];
        }

        provider_state.keys.push(KeyState {
            key_fp: Some(key_fp.to_owned()),
            ..KeyState::default()
        });
        let index = provider_state.keys.len() - 1;
        &mut provider_state.keys[index]
    }

    pub fn mark_exhausted_for(&mut self, provider: &str, key_idx: usize, key_fp: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.key_state_mut_for(provider, key_idx, key_fp)
            .exhausted_at = Some(now);
        self.save();
    }

    pub fn mark_permanently_expired_for(&mut self, provider: &str, key_idx: usize, key_fp: &str) {
        self.key_state_mut_for(provider, key_idx, key_fp).expired = true;
        self.save();
    }

    pub fn is_blocked_for(&self, provider: &str, _key_idx: usize, key_fp: &str) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let state = match self.providers.get(provider).and_then(|provider_state| {
            provider_state
                .keys
                .iter()
                .find(|state| state.key_fp.as_deref() == Some(key_fp))
        }) {
            Some(state) => state,
            None => return false,
        };

        if state.expired {
            return true;
        }

        state
            .exhausted_at
            .map(|timestamp| now.saturating_sub(timestamp) < EXHAUSTION_TTL_SECS)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_and_detect_exhausted() {
        let mut cache = ProviderStateCache::default();

        assert!(!cache.is_key_exhausted("GROQ_API_KEY", 0));

        cache.mark_exhausted("GROQ_API_KEY", 0);
        assert!(cache.is_key_exhausted("GROQ_API_KEY", 0));
    }

    #[test]
    fn test_available_count() {
        let mut cache = ProviderStateCache::default();
        cache.mark_exhausted("GEMINI_API_KEY", 0);
        cache.mark_exhausted("GEMINI_API_KEY", 1);
        assert_eq!(cache.available_key_count("GEMINI_API_KEY", 4), 2);
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut cache = ProviderStateCache::default();
        cache.mark_exhausted("GROQ_API_KEY", 0);
        cache.mark_exhausted("CEREBRAS_API_KEY", 2);

        let json = serde_json::to_string(&cache).expect("serialize ProviderStateCache in test");
        let loaded: ProviderStateCache =
            serde_json::from_str(&json).expect("deserialize ProviderStateCache in test");

        assert!(loaded.is_key_exhausted("GROQ_API_KEY", 0));
        assert!(loaded.is_key_exhausted("CEREBRAS_API_KEY", 2));
        assert!(!loaded.is_key_exhausted("GEMINI_API_KEY", 0));
    }

    #[test]
    fn test_preflight_with_burned_keys() {
        let mut cache = ProviderStateCache::default();
        for i in 0..4 {
            cache.mark_exhausted("GROQ_API_KEY", i);
        }
        for i in 0..4 {
            cache.mark_exhausted("GEMINI_API_KEY", i);
        }
        for i in 0..4 {
            cache.mark_exhausted("CEREBRAS_API_KEY", i);
        }

        let providers = vec![
            ("GROQ_API_KEY", 4),
            ("GEMINI_API_KEY", 4),
            ("CEREBRAS_API_KEY", 4),
            ("OPENROUTER_API_KEY", 1),
            ("GITHUB_TOKEN", 1),
        ];

        let remaining = cache.estimated_remaining_calls(&providers);
        assert_eq!(remaining, 100);
    }

    #[test]
    fn bug_expired_slot_does_not_block_a_different_key() {
        let mut cache = ProviderStateCache::default();
        cache.mark_permanently_expired_for("PROV", 0, "fp-dead");
        assert!(
            !cache.is_blocked_for("PROV", 0, "fp-fresh"),
            "a replacement key in an expired slot inherited the old block"
        );
    }

    #[test]
    fn bug_legacy_slot_block_without_fingerprint_is_not_applied() {
        let mut cache = ProviderStateCache::default();
        cache.mark_permanently_expired("PROV", 0);
        assert!(
            !cache.is_blocked_for("PROV", 0, "fp-current"),
            "legacy index-only block was applied without key identity"
        );
    }

    #[test]
    fn guard_same_key_fingerprint_stays_blocked() {
        let mut cache = ProviderStateCache::default();
        cache.mark_permanently_expired_for("PROV", 0, "fp-dead");
        assert!(cache.is_blocked_for("PROV", 7, "fp-dead"));
    }
}
