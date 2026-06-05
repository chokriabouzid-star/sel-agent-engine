use super::parsers::build_graph_for_files;
use super::{DependencyGraph, GraphLanguage};
use std::path::{Path, PathBuf};

pub fn build_for_workspace(workspace: &Path) -> DependencyGraph {
    let lang = detect_graph_language(workspace);
    if lang == GraphLanguage::Unknown {
        return DependencyGraph::new();
    }

    let files = collect_source_files(workspace, lang);
    if files.is_empty() {
        return DependencyGraph::new();
    }

    build_graph_for_files(&files, workspace, lang)
}

fn detect_graph_language(workspace: &Path) -> GraphLanguage {
    if workspace.join("Cargo.toml").exists() {
        GraphLanguage::Rust
    } else if workspace.join("go.mod").exists() {
        GraphLanguage::Go
    } else if workspace.join("tsconfig.json").exists()
        || workspace.join("package.json").exists()
        || has_files_with_ext(workspace, "ts")
    {
        GraphLanguage::TypeScript
    } else if workspace.join("pyproject.toml").exists()
        || workspace.join("setup.py").exists()
        || workspace.join("requirements.txt").exists()
        || has_files_with_ext(workspace, "py")
    {
        GraphLanguage::Python
    } else {
        GraphLanguage::Unknown
    }
}

fn collect_source_files(workspace: &Path, lang: GraphLanguage) -> Vec<PathBuf> {
    let extensions: &[&str] = match lang {
        GraphLanguage::Python => &["py"],
        GraphLanguage::TypeScript => &["ts", "tsx", "js", "jsx"],
        GraphLanguage::Go => &["go"],
        GraphLanguage::Rust => &["rs"],
        GraphLanguage::Unknown => return vec![],
    };

    let skip_dirs: &[&str] = &[
        "node_modules",
        "venv",
        ".venv",
        "__pycache__",
        ".git",
        "target",
        "dist",
        "build",
        ".tox",
        ".mypy_cache",
    ];

    let mut files = Vec::new();
    walk_source_files(workspace, extensions, skip_dirs, &mut files, 8);
    files
}

fn walk_source_files(
    dir: &Path,
    extensions: &[&str],
    skip_dirs: &[&str],
    out: &mut Vec<PathBuf>,
    max_depth: usize,
) {
    if max_depth == 0 || !dir.is_dir() {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();

        if path.is_dir() {
            let dirname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !skip_dirs.contains(&dirname) {
                walk_source_files(&path, extensions, skip_dirs, out, max_depth - 1);
            }
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if extensions.contains(&ext) {
                out.push(path);
            }
        }
    }
}

fn has_files_with_ext(dir: &Path, ext: &str) -> bool {
    std::fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries.filter_map(|e| e.ok()).any(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x == ext)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
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
    fn builds_python_graph() {
        let dir = setup(&[
            ("main.py", "from utils import helper\n"),
            ("utils.py", "def helper(): pass\n"),
        ]);
        let graph = build_for_workspace(dir.path());
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn builds_typescript_graph() {
        let dir = setup(&[
            ("main.ts", "import { foo } from './utils';\n"),
            ("utils.ts", "export function foo() {}\n"),
        ]);
        let graph = build_for_workspace(dir.path());
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn builds_go_graph() {
        let dir = setup(&[
            ("go.mod", "module myapp\n\ngo 1.21\n"),
            ("main.go", "package main\n\nimport \"myapp/utils\"\n"),
            ("utils/utils.go", "package utils\n"),
        ]);
        let graph = build_for_workspace(dir.path());
        assert!(graph.node_count() >= 2);
        assert!(graph.edge_count() >= 1);
    }

    #[test]
    fn builds_rust_graph() {
        let dir = setup(&[
            ("Cargo.toml", "[package]\nname = \"test\"\n"),
            ("src/main.rs", "mod utils;\n"),
            ("src/utils.rs", "pub fn helper() {}\n"),
        ]);
        let graph = build_for_workspace(dir.path());
        assert!(graph.node_count() >= 2);
        assert!(graph.edge_count() >= 1);
    }

    #[test]
    fn unknown_workspace_returns_empty() {
        let dir = setup(&[("readme.md", "# hello\n")]);
        let graph = build_for_workspace(dir.path());
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn skips_node_modules() {
        let dir = setup(&[
            ("main.ts", "import { foo } from './utils';\n"),
            ("utils.ts", "export function foo() {}\n"),
            ("node_modules/pkg/index.ts", "export const X = 1;\n"),
        ]);
        let graph = build_for_workspace(dir.path());
        let has_node_modules = graph
            .nodes
            .keys()
            .any(|p| p.to_string_lossy().contains("node_modules"));
        assert!(!has_node_modules);
    }
}
