use super::LanguageParser;
use crate::dependency_graph::{FileNode, GraphLanguage, ImportRef};
use std::path::{Path, PathBuf};

pub struct RustParser;

impl LanguageParser for RustParser {
    fn language(&self) -> GraphLanguage {
        GraphLanguage::Rust
    }

    fn parse_file(&self, path: &Path, _workspace: &Path) -> Option<FileNode> {
        let content = std::fs::read_to_string(path).ok()?;
        let mut node = FileNode::new(path, GraphLanguage::Rust);

        for line in content.lines() {
            let line = line.trim();

            if line.starts_with("//") {
                continue;
            }

            // mod declarations
            if let Some(rest) = line.strip_prefix("mod ") {
                let name = rest
                    .trim_end_matches(';')
                    .trim_end_matches(&['{', ' '][..])
                    .trim();
                if !name.is_empty() && !line.contains('{') {
                    node.imports
                        .push(ImportRef::new(format!("mod:{}", name), vec![]));
                }
            }

            // use crate::x or use super::x
            if line.starts_with("use crate::") || line.starts_with("use super::") {
                let path_str = line
                    .trim_start_matches("use ")
                    .trim_end_matches(';')
                    .split('{')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                node.imports.push(ImportRef::new(path_str, vec![]));
            }
        }

        Some(node)
    }

    fn resolve_import(&self, raw: &str, from_file: &Path, workspace: &Path) -> Option<PathBuf> {
        let from_dir = from_file.parent()?;

        if let Some(name) = raw.strip_prefix("mod:") {
            // Try same dir: name.rs
            let as_file = from_dir.join(format!("{}.rs", name));
            if as_file.exists() {
                return Some(as_file);
            }
            // Try name/mod.rs
            let as_mod = from_dir.join(name).join("mod.rs");
            if as_mod.exists() {
                return Some(as_mod);
            }
            // Try src/name.rs
            let src_file = workspace.join("src").join(format!("{}.rs", name));
            if src_file.exists() {
                return Some(src_file);
            }
        }

        None
    }
}
