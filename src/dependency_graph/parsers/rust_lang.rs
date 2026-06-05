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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
        dir
    }

    #[test]
    fn parses_mod_declaration() {
        let dir = setup(&[
            ("src/main.rs", "mod utils;\nmod config;\n"),
            ("src/utils.rs", "pub fn helper() {}"),
            ("src/config.rs", "pub fn load() {}"),
        ]);
        let parser = RustParser;
        let node = parser
            .parse_file(&dir.path().join("src/main.rs"), dir.path())
            .unwrap();
        let raws: Vec<&str> = node.imports.iter().map(|i| i.raw.as_str()).collect();
        assert!(raws.contains(&"mod:utils"));
        assert!(raws.contains(&"mod:config"));
    }

    #[test]
    fn parses_use_crate() {
        let dir = setup(&[("src/agent.rs", "use crate::executor::SafeExecutor;\n")]);
        let parser = RustParser;
        let node = parser
            .parse_file(&dir.path().join("src/agent.rs"), dir.path())
            .unwrap();
        assert_eq!(node.imports.len(), 1);
        assert_eq!(node.imports[0].raw, "crate::executor::SafeExecutor");
    }

    #[test]
    fn parses_use_super() {
        let dir = setup(&[("src/sub/mod.rs", "use super::helper;\n")]);
        let parser = RustParser;
        let node = parser
            .parse_file(&dir.path().join("src/sub/mod.rs"), dir.path())
            .unwrap();
        assert_eq!(node.imports.len(), 1);
        assert_eq!(node.imports[0].raw, "super::helper");
    }

    #[test]
    fn ignores_inline_mod_block() {
        let dir = setup(&[("src/main.rs", "mod tests {\n    use super::*;\n}\n")]);
        let parser = RustParser;
        let node = parser
            .parse_file(&dir.path().join("src/main.rs"), dir.path())
            .unwrap();
        let mod_imports: Vec<_> = node
            .imports
            .iter()
            .filter(|i| i.raw.starts_with("mod:"))
            .collect();
        assert!(mod_imports.is_empty());
    }

    #[test]
    fn resolves_mod_to_sibling_file() {
        let dir = setup(&[
            ("src/main.rs", "mod utils;\n"),
            ("src/utils.rs", "pub fn helper() {}"),
        ]);
        let parser = RustParser;
        let resolved =
            parser.resolve_import("mod:utils", &dir.path().join("src/main.rs"), dir.path());
        assert_eq!(resolved, Some(dir.path().join("src/utils.rs")));
    }

    #[test]
    fn resolves_mod_to_directory_mod_rs() {
        let dir = setup(&[
            ("src/main.rs", "mod utils;\n"),
            ("src/utils/mod.rs", "pub fn helper() {}"),
        ]);
        let parser = RustParser;
        let resolved =
            parser.resolve_import("mod:utils", &dir.path().join("src/main.rs"), dir.path());
        assert_eq!(resolved, Some(dir.path().join("src/utils/mod.rs")));
    }
}
