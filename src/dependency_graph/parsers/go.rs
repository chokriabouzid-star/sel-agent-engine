use super::LanguageParser;
use crate::dependency_graph::{FileNode, GraphLanguage, ImportRef};
use std::path::{Path, PathBuf};

pub struct GoParser;

impl LanguageParser for GoParser {
    fn language(&self) -> GraphLanguage {
        GraphLanguage::Go
    }

    fn parse_file(&self, path: &Path, workspace: &Path) -> Option<FileNode> {
        let content = std::fs::read_to_string(path).ok()?;
        let mut node = FileNode::new(path, GraphLanguage::Go);

        // Read module name from go.mod
        let module_name = read_go_module(workspace).unwrap_or_default();

        let mut in_import_block = false;
        for line in content.lines() {
            let line = line.trim();

            if line == "import (" {
                in_import_block = true;
                continue;
            }
            if line == ")" && in_import_block {
                in_import_block = false;
                continue;
            }

            if in_import_block || line.starts_with("import ") {
                if let Some(pkg) = extract_go_import(line) {
                    if !module_name.is_empty() && pkg.starts_with(&module_name) {
                        node.imports.push(ImportRef::new(pkg, vec![]));
                    }
                }
            }
        }

        Some(node)
    }

    fn resolve_import(&self, raw: &str, _from_file: &Path, workspace: &Path) -> Option<PathBuf> {
        let module_name = read_go_module(workspace).unwrap_or_default();
        if module_name.is_empty() {
            return None;
        }

        let rel = raw.strip_prefix(&module_name)?.trim_start_matches('/');
        let pkg_dir = workspace.join(rel);

        if pkg_dir.is_dir() {
            // Return first .go file in package dir
            std::fs::read_dir(&pkg_dir)
                .ok()?
                .filter_map(|e| e.ok())
                .find(|e| {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x == "go")
                        .unwrap_or(false)
                })
                .map(|e| e.path())
        } else {
            None
        }
    }
}

fn read_go_module(workspace: &Path) -> Option<String> {
    let content = std::fs::read_to_string(workspace.join("go.mod")).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn extract_go_import(line: &str) -> Option<String> {
    let line = line.trim().trim_matches('"');
    let cleaned = line.trim_start_matches("import ").trim().trim_matches('"');
    if cleaned.is_empty() || cleaned.starts_with("//") {
        return None;
    }
    Some(cleaned.to_string())
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
    fn parses_single_import() {
        let dir = setup(&[
            ("go.mod", "module myapp\n\ngo 1.21\n"),
            ("main.go", "package main\n\nimport \"myapp/utils\"\n"),
            ("utils/utils.go", "package utils\n"),
        ]);
        let parser = GoParser;
        let node = parser
            .parse_file(&dir.path().join("main.go"), dir.path())
            .unwrap();
        assert_eq!(node.imports.len(), 1);
        assert_eq!(node.imports[0].raw, "myapp/utils");
    }

    #[test]
    fn parses_import_block() {
        let dir = setup(&[
            ("go.mod", "module myapp\n\ngo 1.21\n"),
            (
                "main.go",
                "package main\n\nimport (\n\t\"myapp/utils\"\n\t\"myapp/config\"\n)\n",
            ),
            ("utils/utils.go", "package utils\n"),
            ("config/config.go", "package config\n"),
        ]);
        let parser = GoParser;
        let node = parser
            .parse_file(&dir.path().join("main.go"), dir.path())
            .unwrap();
        assert_eq!(node.imports.len(), 2);
        let raws: Vec<&str> = node.imports.iter().map(|i| i.raw.as_str()).collect();
        assert!(raws.contains(&"myapp/utils"));
        assert!(raws.contains(&"myapp/config"));
    }

    #[test]
    fn ignores_stdlib_imports() {
        let dir = setup(&[
            ("go.mod", "module myapp\n\ngo 1.21\n"),
            (
                "main.go",
                "package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n\t\"myapp/utils\"\n)\n",
            ),
        ]);
        let parser = GoParser;
        let node = parser
            .parse_file(&dir.path().join("main.go"), dir.path())
            .unwrap();
        assert_eq!(node.imports.len(), 1);
        assert_eq!(node.imports[0].raw, "myapp/utils");
    }

    #[test]
    fn resolves_package_to_go_file() {
        let dir = setup(&[
            ("go.mod", "module myapp\n\ngo 1.21\n"),
            ("utils/helper.go", "package utils\n"),
        ]);
        let parser = GoParser;
        let resolved =
            parser.resolve_import("myapp/utils", &dir.path().join("main.go"), dir.path());
        assert_eq!(resolved, Some(dir.path().join("utils/helper.go")));
    }

    #[test]
    fn returns_none_without_go_mod() {
        let dir = setup(&[("main.go", "package main\n\nimport \"myapp/utils\"\n")]);
        let parser = GoParser;
        let node = parser
            .parse_file(&dir.path().join("main.go"), dir.path())
            .unwrap();
        assert_eq!(node.imports.len(), 0);
    }
}
