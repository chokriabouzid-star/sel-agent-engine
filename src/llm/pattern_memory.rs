// src/llm/pattern_memory.rs — SPO v2.1: Pattern Memory
// Remembers successful error fixes for faster resolution

use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::path::PathBuf;
use std::collections::HashSet;
use super::quota::{ProviderId, now_unix};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub pattern_hash:    String,
    pub language:        String,
    pub error_signature: String,
    pub solution_hint:   String,
    pub provider_used:   String,
    pub repairs_needed:  u32,
    pub success_count:   u32,
    pub timestamp:       u64,
}

#[derive(Debug, Clone)]
pub struct KnownSolution {
    pub hint:               String,
    pub preferred_provider: Option<ProviderId>,
}

pub struct PatternMemory {
    entries:    Vec<MemoryEntry>,
    cache_path: PathBuf,
}

impl PatternMemory {
    const MAX_ENTRIES: usize = 1_000;
    const EVICT_COUNT: usize = 100;

    pub fn load() -> Self {
        Self::load_from(memory_cache_path())
    }

    pub fn load_from(path: PathBuf) -> Self {
        let entries = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Self { entries, cache_path: path }
    }

    pub fn compute_hash(language: &str, error_text: &str) -> String {
        let first_line = error_text.lines().next().unwrap_or("");
        let input = format!("{}|{}", language.to_lowercase(), first_line);
        let hash = Sha256::digest(input.as_bytes());
        format!("{:x}", hash).chars().take(16).collect()
    }

    pub fn lookup(&self, error_text: &str, language: &str) -> Option<KnownSolution> {
        let hash = Self::compute_hash(language, error_text);

        // Exact hash match
        if let Some(entry) = self.entries.iter()
            .find(|e| e.pattern_hash == hash && e.success_count > 0) {
            return Some(self.to_solution(entry));
        }

        // Fuzzy match via Jaccard on words
        let lang_lower = language.to_lowercase();
        let error_prefix: String = error_text.chars().take(100).collect();
        let threshold = if error_prefix.len() < 50 { 0.65 } else { 0.80 };

        self.entries.iter()
            .filter(|e| e.language.to_lowercase() == lang_lower)
            .filter(|e| e.success_count > 0)
            .filter(|e| {
                let ep: String = e.error_signature.chars().take(100).collect();
                jaccard_similarity(&error_prefix, &ep) >= threshold
            })
            .max_by_key(|e| e.success_count)
            .map(|e| self.to_solution(e))
    }

    pub fn record(&mut self, language: &str, error_sig: &str, hint: &str, provider: &str, repairs: u32) {
        let hash = Self::compute_hash(language, error_sig);

        if let Some(existing) = self.entries.iter_mut().find(|e| e.pattern_hash == hash) {
            existing.success_count += 1;
            existing.timestamp = now_unix();
            self.persist();
            return;
        }

        self.entries.push(MemoryEntry {
            pattern_hash:    hash,
            language:        language.to_string(),
            error_signature: error_sig.chars().take(200).collect(),
            solution_hint:   hint.chars().take(120).collect(),
            provider_used:   provider.to_string(),
            repairs_needed:  repairs,
            success_count:   1,
            timestamp:       now_unix(),
        });

        if self.entries.len() > Self::MAX_ENTRIES {
            self.evict_oldest();
        }
        self.persist();
    }

    fn to_solution(&self, entry: &MemoryEntry) -> KnownSolution {
        KnownSolution {
            hint: format!(
                "Known pattern (solved {} time(s), {} repair(s) avg): {}",
                entry.success_count, entry.repairs_needed, entry.solution_hint
            ),
            preferred_provider: entry.provider_used.parse().ok(),
        }
    }

    fn evict_oldest(&mut self) {
        self.entries.sort_by(|a, b| {
            b.success_count.cmp(&a.success_count)
                .then(b.timestamp.cmp(&a.timestamp))
        });
        self.entries.truncate(Self::MAX_ENTRIES - Self::EVICT_COUNT);
    }

    fn persist(&self) {
        let json = match serde_json::to_string_pretty(&self.entries) {
            Ok(j) => j,
            Err(e) => { eprintln!("[Memory] serialize: {}", e); return; }
        };
        let tmp = self.cache_path.with_extension("tmp");
        if let Some(parent) = tmp.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Err(e) = atomic_write_bytes(&tmp, &self.cache_path, json.as_bytes()) {
            eprintln!("[Memory] persist: {}", e);
        }
    }
}

fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let wa: HashSet<&str> = a.split_whitespace().collect();
    let wb: HashSet<&str> = b.split_whitespace().collect();
    if wa.is_empty() && wb.is_empty() { return 1.0; }
    let inter = wa.intersection(&wb).count();
    let union_count = wa.union(&wb).count();
    if union_count == 0 { return 0.0; }
    inter as f64 / union_count as f64
}

fn memory_cache_path() -> PathBuf {
    let mut p = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("sel-agent");
    std::fs::create_dir_all(&p).ok();
    p.push("pattern_memory.json");
    p
}

fn atomic_write_bytes(tmp: &PathBuf, dest: &PathBuf, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::File::create(tmp).map_err(|e| format!("create: {e}"))?;
    f.write_all(data).map_err(|e| format!("write: {e}"))?;
    f.sync_all().map_err(|e| format!("fsync: {e}"))?;
    std::fs::rename(tmp, dest).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn test_same_error_same_hash() {
        let h1 = PatternMemory::compute_hash("python", "ImportError: No module named 'x'");
        let h2 = PatternMemory::compute_hash("python", "ImportError: No module named 'x'");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_different_lang_different_hash() {
        let h1 = PatternMemory::compute_hash("python", "same error");
        let h2 = PatternMemory::compute_hash("rust",   "same error");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_jaccard() {
        let s = jaccard_similarity("the quick brown fox", "the quick brown dog");
        assert!(s > 0.5 && s < 1.0);
        assert!((jaccard_similarity("", "") - 1.0).abs() < 0.01);
    }
}
