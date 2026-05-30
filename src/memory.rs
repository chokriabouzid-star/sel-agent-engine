#![allow(clippy::empty_docs)]
// src/memory.rs  v7.2.0: Error Fingerprinting
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MEMORY_FILE: &str = ".sel_memory.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub failure_kind: String,
    pub error_signature: String,
    pub normalized_hash: u64,
    pub successful_fix: String,
    pub count: u32,
    pub last_seen: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FailureMemory {
    pub entries: Vec<MemoryEntry>,
}

///
pub fn normalize_error(error: &str) -> String {
    let first_line = error.lines().next().unwrap_or(error);
    let s = first_line.to_lowercase();

    //    N
    let mut out = String::new();
    let mut in_num = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !in_num {
                out.push('N');
                in_num = true;
            }
        } else {
            in_num = false;
            //  single quotes  Q
            if c == '\'' || c == '`' {
                out.push('Q');
            } else {
                out.push(c);
            }
        }
    }

    out.trim().to_string()
}

///  FNV
pub fn hash_normalized(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |acc, b| {
        acc.wrapping_mul(0x100000001b3).wrapping_add(b as u64)
    })
}

//  QuickFix

#[derive(Debug, Clone)]
pub enum QuickFix {
    InstallPackage { command: String },
    AddGoImport { symbol: String },
}

///    LLM
pub fn quick_fix(error: &str) -> Option<QuickFix> {
    let lower = error.to_lowercase();

    // Python: ModuleNotFoundError
    if lower.contains("modulenotfounderror") || lower.contains("no module named") {
        let module = extract_module_name(error)?;
        let pkg = match module.as_str() {
            "fastapi" => "fastapi uvicorn",
            "uvicorn" => "uvicorn[standard]",
            "sqlalchemy" => "sqlalchemy",
            "pydantic" => "pydantic",
            "jose" => "python-jose",
            "passlib" => "passlib bcrypt",
            "dotenv" => "python-dotenv",
            "httpx" => "httpx",
            "pytest" => "pytest",
            "requests" => "requests",
            "flask" => "flask",
            _ => return None,
        };
        return Some(QuickFix::InstallPackage {
            command: format!("venv/bin/pip install {}", pkg),
        });
    }

    // Go: undefined: fmt
    if lower.contains("undefined:") {
        let sym = extract_go_undefined(error)?;
        let go_std = [
            "fmt", "errors", "strings", "strconv", "sort", "math", "os", "io", "log", "time",
            "sync", "context", "bytes", "bufio",
        ];
        if go_std.contains(&sym.as_str()) {
            return Some(QuickFix::AddGoImport { symbol: sym });
        }
    }

    None
}

fn extract_module_name(error: &str) -> Option<String> {
    let lower = error.to_lowercase();
    for prefix in &["no module named '", "no module named \""] {
        if let Some(idx) = lower.find(prefix) {
            let after = &error[idx + prefix.len()..];
            let name = after.split(['.', '\'', '"']).next()?.trim();
            if !name.is_empty() {
                return Some(name.to_lowercase());
            }
        }
    }
    None
}

fn extract_go_undefined(error: &str) -> Option<String> {
    for line in error.lines() {
        if line.contains("undefined:") {
            let after = line.split("undefined:").nth(1)?;
            let sym = after.split_whitespace().next()?.trim();
            if !sym.is_empty() {
                return Some(sym.to_string());
            }
        }
    }
    None
}

//  FailureMemory

fn memory_path() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(MEMORY_FILE)
    } else {
        PathBuf::from(MEMORY_FILE)
    }
}

fn today_str() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("day-{}", secs / 86400)
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

    ///  repair    normalized_hash
    pub fn record_success(&mut self, failure_kind: &str, error_sig: &str, fix_summary: &str) {
        let normalized = normalize_error(error_sig);
        let hash = hash_normalized(&normalized);

        if let Some(entry) = self.entries.iter_mut().find(|e| e.normalized_hash == hash) {
            entry.count += 1;
            entry.last_seen = today_str();
            entry.successful_fix = fix_summary.chars().take(200).collect();
        } else {
            self.entries.push(MemoryEntry {
                failure_kind: failure_kind.to_string(),
                error_signature: normalized,
                normalized_hash: hash,
                successful_fix: fix_summary.chars().take(200).collect(),
                count: 1,
                last_seen: today_str(),
            });
        }

        if self.entries.len() > 100 {
            self.entries.sort_by_key(|b| std::cmp::Reverse(b.count));
            self.entries.truncate(100);
        }
        self.save();
    }

    ///  hints  normalized_hash
    pub fn get_hints(&self, failure_kind: &str, error_sig: &str) -> String {
        let hash = hash_normalized(&normalize_error(error_sig));

        let relevant: Vec<&MemoryEntry> = self
            .entries
            .iter()
            .filter(|e| e.normalized_hash == hash || e.failure_kind == failure_kind)
            .collect();

        if relevant.is_empty() {
            return String::new();
        }

        let mut hints = String::from("\n\nFAILURE MEMORY:\n");
        for entry in relevant.iter().take(3) {
            hints.push_str(&format!(
                "- [{}x] {}: {}\n",
                entry.count, entry.failure_kind, entry.successful_fix
            ));
        }
        hints
    }
}
