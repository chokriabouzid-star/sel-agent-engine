pub struct KeyPool {
    pub keys: Vec<String>,
    current: usize,
    exhausted: std::collections::HashSet<usize>,
    prefix: String,
}

impl KeyPool {
    pub fn from_env(prefix: &str) -> Self {
        let mut keys = Vec::new();

        // 1. Base key: e.g. GROQ_API_KEY
        if let Ok(key) = std::env::var(prefix) {
            if !key.trim().is_empty() { keys.push(key); }
        }

        for i in 1..=10 {
            // 2. Standard format:  GROQ_API_KEY_1, GROQ_API_KEY_2, 
            let var_underscore = format!("{}_{}", prefix, i);
            if let Ok(key) = std::env::var(&var_underscore) {
                if !key.trim().is_empty() && !keys.contains(&key) {
                    keys.push(key);
                }
            }
            // 3. No-underscore format (common user mistake): GROQ_API_KEY1
            let var_nounderscore = format!("{}{}", prefix, i);
            if let Ok(key) = std::env::var(&var_nounderscore) {
                if !key.trim().is_empty() && !keys.contains(&key) {
                    keys.push(key);
                }
            }
            // 4. Short prefix format: GROQ_1 (e.g. when user sets GROQ_1 instead of GROQ_API_KEY_1)
            // Derive short prefix: GROQ_API_KEY  GROQ, CEREBRAS_API_KEY  CEREBRAS
            let short = prefix.replace("_API_KEY", "").replace("_TOKEN", "");
            if short != prefix {
                let var_short = format!("{}_{}", short, i);
                if let Ok(key) = std::env::var(&var_short) {
                    if !key.trim().is_empty() && !keys.contains(&key) {
                        keys.push(key);
                    }
                }
            }
        }

        
        let mut exhausted = std::collections::HashSet::new();
        // v7.9.9 P2: Load from disk cache
        let cache = crate::provider_state::ProviderStateCache::load();
        for i in 0..keys.len() {
            if cache.is_key_exhausted(prefix, i) {
                exhausted.insert(i);
                eprintln!(
                    "   🔑 Key #{} for {} pre-skipped (exhausted in previous session)",
                    i + 1, prefix
                );
            }
        }
        
        Self { keys, current: 0, exhausted, prefix: prefix.to_string() }
    }

    /// Returns the next available key
    pub fn next_available(&mut self) -> Option<&str> {
        if self.keys.is_empty() {
            return None;
        }
        let total = self.keys.len();
        for _ in 0..total {
            if !self.exhausted.contains(&self.current) {
                return Some(&self.keys[self.current]);
            }
            self.current = (self.current + 1) % total;
        }
        None // All keys exhausted
    }

    pub fn mark_expired(&mut self) {
        if self.keys.is_empty() { return; }
        eprintln!(
            " ❌ Key #{} for {} PERMANENTLY EXPIRED  removed from rotation",
            self.current + 1, self.prefix
        );
        self.exhausted.insert(self.current);
        
        let mut cache = crate::provider_state::ProviderStateCache::load();
        cache.mark_permanently_expired(&self.prefix, self.current);
        
        self.current = (self.current + 1) % self.keys.len();
    }

    /// Mark the current key as daily exhausted
    pub fn mark_exhausted(&mut self) {
        if self.keys.is_empty() { return; }
        eprintln!(
            " 🔄 Key #{} for {} exhausted  rotating to next key",
            self.current + 1, self.prefix
        );
        self.exhausted.insert(self.current);
        
        // v7.9.9 P2: Save to disk cache
        let mut cache = crate::provider_state::ProviderStateCache::load();
        cache.mark_exhausted(&self.prefix, self.current);
        
        self.current = (self.current + 1) % self.keys.len();
    }

    pub fn has_available(&self) -> bool {
        self.exhausted.len() < self.keys.len() && !self.keys.is_empty()
    }
}
