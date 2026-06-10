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

            if line.is_empty() || line.starts_with("//") {
                continue;
            }

            // mod / pub mod declarations
            if let Some(name) = parse_mod_declaration(line) {
                node.imports
                    .push(ImportRef::new(format!("mod:{}", name), vec![]));
            }

            // use / pub use paths
            if let Some(path_str) = parse_use_path(line) {
                node.imports.push(ImportRef::new(path_str, vec![]));
            }
        }

        Some(node)
    }

    fn resolve_import(&self, raw: &str, from_file: &Path, workspace: &Path) -> Option<PathBuf> {
        if let Some(name) = raw.strip_prefix("mod:") {
            return resolve_mod_name(name, from_file, workspace);
        }

        if let Some(rest) = raw.strip_prefix("crate::") {
            let src_root = workspace.join("src");
            let segments: Vec<&str> = rest.split("::").filter(|s| !s.is_empty()).collect();
            return resolve_from_base(&src_root, &segments);
        }

        if let Some(rest) = raw.strip_prefix("self::") {
            let base = current_module_dir(from_file)?;
            let segments: Vec<&str> = rest.split("::").filter(|s| !s.is_empty()).collect();
            return resolve_from_base(&base, &segments);
        }

        if raw.starts_with("super::") {
            let mut rest = raw;
            let mut base = current_module_dir(from_file)?;

            while let Some(stripped) = rest.strip_prefix("super::") {
                base = base.parent()?.to_path_buf();
                rest = stripped;
            }

            let segments: Vec<&str> = rest.split("::").filter(|s| !s.is_empty()).collect();
            return resolve_from_base(&base, &segments);
        }

        None
    }
}

fn parse_mod_declaration(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("pub mod ")
        .or_else(|| line.strip_prefix("mod "))?;

    // Ignore inline module blocks: mod tests { ... }
    if line.contains('{') {
        return None;
    }

    let name = rest.trim_end_matches(';').trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn parse_use_path(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("pub use ")
        .or_else(|| line.strip_prefix("use "))?;

    let raw = rest
        .trim_end_matches(';')
        .split(" as ")
        .next()
        .unwrap_or("")
        .split('{')
        .next()
        .unwrap_or("")
        .trim();

    if raw.is_empty() || raw.ends_with("::") {
        return None;
    }

    if raw.starts_with("crate::") || raw.starts_with("super::") || raw.starts_with("self::") {
        Some(raw.to_string())
    } else {
        None
    }
}

fn current_module_dir(from_file: &Path) -> Option<PathBuf> {
    let parent = from_file.parent()?;
    let filename = from_file.file_name()?.to_str()?;

    match filename {
        "main.rs" | "lib.rs" | "mod.rs" => Some(parent.to_path_buf()),
        _ => {
            let stem = from_file.file_stem()?.to_str()?;
            Some(parent.join(stem))
        }
    }
}

fn resolve_mod_name(name: &str, from_file: &Path, workspace: &Path) -> Option<PathBuf> {
    let from_dir = from_file.parent()?;

    for base in [from_dir.to_path_buf(), workspace.join("src")] {
        let as_file = base.join(format!("{}.rs", name));
        if as_file.exists() {
            return Some(as_file);
        }

        let as_mod = base.join(name).join("mod.rs");
        if as_mod.exists() {
            return Some(as_mod);
        }
    }

    None
}

fn resolve_from_base(base: &Path, segments: &[&str]) -> Option<PathBuf> {
    if segments.is_empty() {
        return None;
    }

    // Try the longest plausible module prefix first, then shorten.
    for len in (1..=segments.len()).rev() {
        let mut candidate = base.to_path_buf();
        for seg in &segments[..len] {
            candidate.push(seg);
        }

        let as_file = candidate.with_extension("rs");
        if as_file.exists() {
            return Some(as_file);
        }

        let as_mod = candidate.join("mod.rs");
        if as_mod.exists() {
            return Some(as_mod);
        }
    }

    None
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
    fn parses_pub_mod_declaration() {
        let dir = setup(&[("src/lib.rs", "pub mod builder;\npub mod parsers;\n")]);
        let parser = RustParser;
        let node = parser
            .parse_file(&dir.path().join("src/lib.rs"), dir.path())
            .unwrap();
        let raws: Vec<&str> = node.imports.iter().map(|i| i.raw.as_str()).collect();
        assert!(raws.contains(&"mod:builder"));
        assert!(raws.contains(&"mod:parsers"));
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
    fn parses_pub_use_self() {
        let dir = setup(&[("src/executor/mod.rs", "pub use self::core::SafeExecutor;\n")]);
        let parser = RustParser;
        let node = parser
            .parse_file(&dir.path().join("src/executor/mod.rs"), dir.path())
            .unwrap();
        assert_eq!(node.imports.len(), 1);
        assert_eq!(node.imports[0].raw, "self::core::SafeExecutor");
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

    #[test]
    fn resolves_crate_to_mod_rs() {
        let dir = setup(&[
            ("src/agent.rs", "use crate::executor::SafeExecutor;\n"),
            ("src/executor/mod.rs", "pub use self::core::SafeExecutor;\n"),
            ("src/executor/core.rs", "pub struct SafeExecutor;\n"),
        ]);
        let parser = RustParser;
        let resolved = parser.resolve_import(
            "crate::executor::SafeExecutor",
            &dir.path().join("src/agent.rs"),
            dir.path(),
        );
        assert_eq!(resolved, Some(dir.path().join("src/executor/mod.rs")));
    }

    #[test]
    fn resolves_crate_to_nested_file() {
        let dir = setup(&[
            (
                "src/agent.rs",
                "use crate::dependency_graph::builder::build_for_workspace;\n",
            ),
            ("src/dependency_graph/mod.rs", "pub mod builder;\n"),
            (
                "src/dependency_graph/builder.rs",
                "pub fn build_for_workspace() {}\n",
            ),
        ]);
        let parser = RustParser;
        let resolved = parser.resolve_import(
            "crate::dependency_graph::builder::build_for_workspace",
            &dir.path().join("src/agent.rs"),
            dir.path(),
        );
        assert_eq!(
            resolved,
            Some(dir.path().join("src/dependency_graph/builder.rs"))
        );
    }

    #[test]
    fn resolves_self_to_sibling_file() {
        let dir = setup(&[
            ("src/executor/mod.rs", "pub use self::core::SafeExecutor;\n"),
            ("src/executor/core.rs", "pub struct SafeExecutor;\n"),
        ]);
        let parser = RustParser;
        let resolved = parser.resolve_import(
            "self::core::SafeExecutor",
            &dir.path().join("src/executor/mod.rs"),
            dir.path(),
        );
        assert_eq!(resolved, Some(dir.path().join("src/executor/core.rs")));
    }

    #[test]
    fn resolves_super_from_nested_rs() {
        let dir = setup(&[
            ("src/sub/nested.rs", "use super::helper;\n"),
            ("src/sub/helper.rs", "pub fn helper() {}\n"),
        ]);
        let parser = RustParser;
        let resolved = parser.resolve_import(
            "super::helper",
            &dir.path().join("src/sub/nested.rs"),
            dir.path(),
        );
        assert_eq!(resolved, Some(dir.path().join("src/sub/helper.rs")));
    }
}
