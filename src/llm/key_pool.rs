pub struct KeyPool {
    pub keys: Vec<String>,
    current: usize,
    exhausted: std::collections::HashSet<usize>,
}

impl KeyPool {
    pub fn from_env(prefix: &str) -> Self {
        let mut keys = Vec::new();
        // Fallback for the base key (e.g., GROQ_API_KEY)
        if let Ok(key) = std::env::var(prefix) {
            keys.push(key);
        }
        // Check for multiple keys (e.g., GROQ_API_KEY_1, GROQ_API_KEY_2)
        for i in 1..=10 {
            let var = format!("{}_{}", prefix, i);
            if let Ok(key) = std::env::var(&var) {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        Self { keys, current: 0, exhausted: Default::default() }
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

    /// Mark the current key as daily exhausted
    pub fn mark_exhausted(&mut self) {
        if self.keys.is_empty() { return; }
        eprintln!(
            "🔑 Key #{} exhausted → rotating to next key",
            self.current + 1
        );
        self.exhausted.insert(self.current);
        self.current = (self.current + 1) % self.keys.len();
    }

    pub fn has_available(&self) -> bool {
        self.exhausted.len() < self.keys.len() && !self.keys.is_empty()
    }
}
