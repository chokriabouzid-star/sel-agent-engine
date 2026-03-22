// src/memory.rs — v5.8: Failure Memory
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MEMORY_FILE: &str = ".sel_memory.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub failure_kind:    String,   // "BuildError", "SyntaxError", etc.
    pub error_signature: String,   // أول 120 حرف من الخطأ
    pub successful_fix:  String,   // وصف الإصلاح الناجح
    pub count:           u32,      // عدد مرات التكرار
    pub last_seen:       String,   // تاريخ آخر ظهور
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FailureMemory {
    pub entries: Vec<MemoryEntry>,
}

fn memory_path() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(MEMORY_FILE)
    } else {
        PathBuf::from(MEMORY_FILE)
    }
}

impl FailureMemory {
    pub fn load() -> Self {
        let path = memory_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let path = memory_path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    // حفظ repair ناجح
    pub fn record_success(&mut self, failure_kind: &str, error_sig: &str, fix_summary: &str) {
        let sig = error_sig.chars().take(120).collect::<String>();
        let today = {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let days = secs / 86400;
            format!("day-{}", days)
        };

        // هل موجود مسبقاً؟
        if let Some(entry) = self.entries.iter_mut().find(|e| {
            e.failure_kind == failure_kind && e.error_signature == sig
        }) {
            entry.count += 1;
            entry.last_seen = today;
            entry.successful_fix = fix_summary.chars().take(200).collect();
        } else {
            self.entries.push(MemoryEntry {
                failure_kind:    failure_kind.to_string(),
                error_signature: sig,
                successful_fix:  fix_summary.chars().take(200).collect::<String>(),
                count:           1,
                last_seen:       today,
            });
        }
        // احتفظ بآخر 100 entry فقط
        if self.entries.len() > 100 {
            self.entries.sort_by(|a, b| b.count.cmp(&a.count));
            self.entries.truncate(100);
        }
        self.save();
    }

    // جلب hints ذات الصلة بنوع الفشل الحالي
    pub fn get_hints(&self, failure_kind: &str, error_sig: &str) -> String {
        let sig_short = error_sig.chars().take(80).collect::<String>();
        let relevant: Vec<&MemoryEntry> = self.entries.iter()
            .filter(|e| {
                e.failure_kind == failure_kind ||
                e.error_signature.contains(&sig_short[..sig_short.len().min(40)])
            })
            .collect();

        if relevant.is_empty() {
            return String::new();
        }

        let mut hints = String::from("\n\nFAILURE MEMORY (learned from past repairs):\n");
        for entry in relevant.iter().take(3) {
            hints.push_str(&format!(
                "- [{}x] {}: {}\n",
                entry.count, entry.failure_kind, entry.successful_fix
            ));
        }
        hints
    }
}
