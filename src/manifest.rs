// src/manifest.rs — v7.3: Project Manifest
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub language: String,
    pub files:    Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path:    String,
    pub kind:    FileKind,
    pub exports: Vec<String>,
    pub imports: Vec<String>, // خفيف — أسماء فقط
    pub size:    usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileKind { Source, Test, Config }

/// v7.4: File ownership policy — enforced in executor.rs
#[derive(Debug, Clone, PartialEq)]
pub enum FilePolicy {
    Mutable,    // source — LLM writes and modifies
    ReadOnly,   // test/spec — LLM reads only
    Protected,  // config/scaffold — fully protected
}

impl FileEntry {
    pub fn policy(&self) -> FilePolicy {
        match self.kind {
            FileKind::Test   => FilePolicy::ReadOnly,
            FileKind::Config => FilePolicy::Protected,
            FileKind::Source => FilePolicy::Mutable,
        }
    }
}

impl ProjectManifest {
    pub fn generate(workspace: &Path) -> Self {
        let profile  = crate::context::Scanner::scan(workspace);
        let language = format!("{}", profile.language);
        let files    = Self::scan_files(workspace);
        Self { language, files }
    }

    fn scan_files(workspace: &Path) -> Vec<FileEntry> {
        let supported = ["ts","js","py","go","rs","toml","json"];
        let mut entries: Vec<FileEntry> = walkdir::WalkDir::new(workspace)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter(|e| {
                let ext = e.path().extension()
                    .and_then(|s| s.to_str()).unwrap_or("");
                supported.contains(&ext)
            })
            .filter(|e| {
                let s = e.path().to_string_lossy();
                !s.contains("node_modules")
                    && !s.contains("/venv/")
                    && !s.contains("/dist/")
                    && !s.contains("/target/")
                    && !s.contains("/.")
                    && !s.contains("package-lock")
            })
            .filter_map(|e| Self::analyze_file(workspace, e.path()))
            .collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        entries
    }

    fn analyze_file(workspace: &Path, path: &Path) -> Option<FileEntry> {
        let rel  = path.strip_prefix(workspace).ok()?
                       .to_string_lossy().to_string();
        let content = fs::read_to_string(path).ok()?;
        let size    = content.lines().count();
        let ext     = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        let kind = if rel.contains(".test.") || rel.contains("_test.")
                      || rel.starts_with("test_") || rel.contains("/test_") {
            FileKind::Test
        } else if matches!(ext, "json" | "toml") {
            FileKind::Config
        } else {
            FileKind::Source
        };

        let exports = if kind == FileKind::Source {
            Self::extract_exports(&content, ext)
        } else { vec![] };

        let imports = Self::extract_imports(&content, ext);

        Some(FileEntry { path: rel, kind, exports, imports, size })
    }

    // ─── Exports ───────────────────────────────────────────
    fn extract_exports(content: &str, ext: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in content.lines() {
            let t = line.trim();
            match ext {
                "ts" | "js" => {
                    if let Some(rest) = t.strip_prefix("export ") {
                        if let Some(n) = Self::first_ident(rest) {
                            // تجاهل keywords
                            if !matches!(n.as_str(), "default"|"type"|"interface"|"{") {
                                out.push(n);
                            }
                        }
                    }
                }
                "py" => {
                    // دوال وكلاسات على مستوى أعلى (بدون indent)
                    if !line.starts_with(' ') && !line.starts_with('\t')
                        && (t.starts_with("def ") || t.starts_with("class ")) {
                            if let Some(n) = Self::ident_after_keyword(t) {
                                if !n.starts_with('_') { out.push(n); }
                            }
                        }
                }
                "rs" => {
                    if t.starts_with("pub fn ")
                        || t.starts_with("pub struct ")
                        || t.starts_with("pub enum ")
                        || t.starts_with("pub trait ") {
                        if let Some(n) = Self::ident_after_pub(t) { out.push(n); }
                    }
                }
                "go" => {
                    if t.starts_with("func ") || t.starts_with("type ") {
                        if let Some(n) = Self::extract_go_export(t) { out.push(n); }
                    }
                }
                _ => {}
            }
        }
        out.dedup();
        out
    }

    // ─── Imports (خفيف) ─────────────────────────────────────
    fn extract_imports(content: &str, ext: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in content.lines() {
            let t = line.trim();
            match ext {
                "ts" | "js" => {
                    // import ... from "./database"
                    if t.starts_with("import ") {
                        if let Some(from) = t.rfind("from ") {
                            let src = t[from+5..].trim().trim_matches(|c| c=='\''||c=='"'||c==';');
                            if !src.is_empty() { out.push(src.to_string()); }
                        }
                    }
                }
                "py" => {
                    // from .database import X  or  import os
                    if t.starts_with("from ") || t.starts_with("import ") {
                        let parts: Vec<&str> = t.split_whitespace().collect();
                        if parts.len() >= 2 {
                            out.push(parts[1].trim_end_matches(',').to_string());
                        }
                    }
                }
                "go" => {
                    // import "fmt"
                    if t.starts_with('"') && t.ends_with('"') {
                        out.push(t.trim_matches('"').to_string());
                    }
                }
                "rs" => {
                    // use crate::X;  use std::...
                    if t.starts_with("use ") {
                        let src = t[4..].trim_end_matches(';')
                            .split("::").next().unwrap_or("").to_string();
                        if !src.is_empty() { out.push(src); }
                    }
                }
                _ => {}
            }
        }
        out.dedup();
        out
    }

    // ─── Helpers ────────────────────────────────────────────
    fn first_ident(s: &str) -> Option<String> {
        // "function add(" → "add"
        // "class User"    → "User"
        // "const PI"      → "PI"
        let words: Vec<&str> = s.split_whitespace().collect();
        let start = if matches!(words.first(), Some(&"function")|Some(&"class")
                                |Some(&"const")|Some(&"let")|Some(&"var")
                                |Some(&"async")) { 1 } else { 0 };
        words.get(start).map(|w| {
            w.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_')
             .to_string()
        })
    }

    fn ident_after_keyword(line: &str) -> Option<String> {
        // "def add():" → "add"
        // "def get_all(store):" → "get_all"
        line.split_whitespace().nth(1)
            .map(|w| {
                // قطع عند أول ( أو : أو )
                let end = w.find(['(', ':', ')'])
                    .unwrap_or(w.len());
                w[..end].to_string()
            })
    }

    fn ident_after_pub(line: &str) -> Option<String> {
        // "pub fn add(" → "add"
        // "pub struct User" → "User"
        line.split_whitespace().nth(2)
            .map(|w| w.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_').to_string())
    }

    fn extract_go_export(line: &str) -> Option<String> {
        // "func Add(" → "Add"  (capital = exported)
        // "type User struct" → "User"
        let parts: Vec<&str> = line.split_whitespace().collect();
        let name = parts.get(1)?
            .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if name.chars().next()?.is_uppercase() {
            Some(name.to_string())
        } else {
            None
        }
    }

    // ─── Summary للـ LLM ────────────────────────────────────
    pub fn to_summary(&self) -> String {
        if self.files.is_empty() { return String::new(); }

        let mut s = String::from("PROJECT MANIFEST:\n");
        s.push_str(&format!("  Language: {}\n", self.language));
        s.push_str("  Files:\n");

        for f in &self.files {
            let k = match f.kind {
                FileKind::Source => "src",
                FileKind::Test   => "test",
                FileKind::Config => "cfg",
            };
            s.push_str(&format!("    [{k}] {} ({} lines)", f.path, f.size));
            if !f.exports.is_empty() {
                s.push_str(&format!(" exports:[{}]", f.exports.join(",")));
            }
            if !f.imports.is_empty() {
                s.push_str(&format!(" imports:[{}]", f.imports.join(",")));
            }
            s.push('\n');
        }
        s
    }
}
