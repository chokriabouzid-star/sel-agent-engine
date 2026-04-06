// src/file_snapshot.rs — FileSnapshot v1.0
// حالة نظام الملفات الحقيقية في لحظة معينة

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path:     String,
    pub size:     u64,
    pub modified: u64, // unix seconds
    pub hash:     u32, // FNV-1a سريع بدون dependency
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub files:        HashMap<String, FileInfo>,
    pub timestamp:    u64,
    pub project_root: String,
}

#[derive(Debug, Clone)]
pub struct SnapshotDiff {
    pub added:    Vec<String>,
    pub modified: Vec<String>,
    pub deleted:  Vec<String>,
}

impl SnapshotDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    pub fn summary(&self) -> String {
        let mut parts = vec![];
        if !self.added.is_empty() {
            parts.push(format!("+{} added", self.added.len()));
        }
        if !self.modified.is_empty() {
            parts.push(format!("~{} modified", self.modified.len()));
        }
        if !self.deleted.is_empty() {
            parts.push(format!("-{} deleted", self.deleted.len()));
        }
        if parts.is_empty() {
            "no changes".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// FNV-1a hash — سريع وبدون dependency خارجية
fn fnv1a(data: &[u8]) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for &byte in data {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

fn unix_modified(path: &Path) -> u64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// الامتدادات التي نتتبعها
fn is_tracked(path: &Path) -> bool {
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    matches!(ext,
        "py" | "js" | "ts" | "rs" | "go" |
        "toml" | "json" | "yaml" | "yml" |
        "md" | "txt" | "sh" | "env" |
        "html" | "css" | "sql"
    )
}

/// المجلدات التي نتجاهلها
fn is_ignored(path: &Path) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_str().unwrap_or("");
        matches!(s,
            "target" | "node_modules" | ".git" |
            "venv" | "__pycache__" | ".pytest_cache" |
            "dist" | "build" | ".agent"
        )
    })
}

impl FileSnapshot {
    /// أخذ مسح كامل للمجلد
    pub fn take(root: &Path) -> Self {
        let mut files = HashMap::new();
        let timestamp = now_unix();

        Self::scan_dir(root, root, &mut files);

        FileSnapshot {
            files,
            timestamp,
            project_root: root.to_string_lossy().to_string(),
        }
    }

    fn scan_dir(root: &Path, dir: &Path, files: &mut HashMap<String, FileInfo>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if is_ignored(&path) {
                continue;
            }

            if path.is_dir() {
                Self::scan_dir(root, &path, files);
            } else if path.is_file() && is_tracked(&path) {
                let rel = path.strip_prefix(root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());

                let content = std::fs::read(&path).unwrap_or_default();
                let size     = content.len() as u64;
                let hash     = fnv1a(&content);
                let modified = unix_modified(&path);

                files.insert(rel.clone(), FileInfo {
                    path: rel,
                    size,
                    modified,
                    hash,
                });
            }
        }
    }

    /// مقارنة مع snapshot سابق
    pub fn diff(&self, before: &FileSnapshot) -> SnapshotDiff {
        let mut added    = vec![];
        let mut modified = vec![];
        let mut deleted  = vec![];

        // ملفات جديدة أو معدّلة
        for (path, info) in &self.files {
            match before.files.get(path) {
                None => added.push(path.clone()),
                Some(old) if old.hash != info.hash => modified.push(path.clone()),
                _ => {}
            }
        }

        // ملفات محذوفة
        for path in before.files.keys() {
            if !self.files.contains_key(path) {
                deleted.push(path.clone());
            }
        }

        added.sort();
        modified.sort();
        deleted.sort();

        SnapshotDiff { added, modified, deleted }
    }

    /// قائمة مسارات الملفات فقط — للإرسال للنموذج
    pub fn file_list(&self) -> Vec<String> {
        let mut list: Vec<String> = self.files.keys().cloned().collect();
        list.sort();
        list
    }

    /// عدد الملفات
    pub fn count(&self) -> usize {
        self.files.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_empty_dir() {
        let dir = tempdir().unwrap();
        let snap = FileSnapshot::take(dir.path());
        assert_eq!(snap.count(), 0);
        assert!(snap.diff(&snap.clone()).is_empty());
    }

    #[test]
    fn test_detects_new_file() {
        let dir  = tempdir().unwrap();
        let before = FileSnapshot::take(dir.path());

        fs::write(dir.path().join("main.py"), "x = 1").unwrap();
        let after = FileSnapshot::take(dir.path());

        let diff = after.diff(&before);
        assert_eq!(diff.added, vec!["main.py"]);
        assert!(diff.modified.is_empty());
        assert!(diff.deleted.is_empty());
    }

    #[test]
    fn test_detects_modified_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("app.py"), "x = 1").unwrap();
        let before = FileSnapshot::take(dir.path());

        fs::write(dir.path().join("app.py"), "x = 2").unwrap();
        let after = FileSnapshot::take(dir.path());

        let diff = after.diff(&before);
        assert!(diff.added.is_empty());
        assert_eq!(diff.modified, vec!["app.py"]);
    }

    #[test]
    fn test_detects_deleted_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("old.py"), "pass").unwrap();
        let before = FileSnapshot::take(dir.path());

        fs::remove_file(dir.path().join("old.py")).unwrap();
        let after = FileSnapshot::take(dir.path());

        let diff = after.diff(&before);
        assert_eq!(diff.deleted, vec!["old.py"]);
    }

    #[test]
    fn test_ignores_target_dir() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("target").join("main.rs"), "fn main(){}").unwrap();
        fs::write(dir.path().join("lib.rs"), "pub fn x(){}").unwrap();

        let snap = FileSnapshot::take(dir.path());
        assert_eq!(snap.count(), 1);
        assert!(snap.files.contains_key("lib.rs"));
    }

    #[test]
    fn test_summary_format() {
        let diff = SnapshotDiff {
            added:    vec!["a.py".into()],
            modified: vec!["b.py".into(), "c.py".into()],
            deleted:  vec![],
        };
        let s = diff.summary();
        assert!(s.contains("+1"));
        assert!(s.contains("~2"));
    }

    #[test]
    fn test_fnv1a_different_content() {
        let h1 = fnv1a(b"hello");
        let h2 = fnv1a(b"world");
        assert_ne!(h1, h2);
    }
}
