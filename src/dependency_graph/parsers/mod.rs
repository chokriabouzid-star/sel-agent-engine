pub mod go;
mod python;
pub mod rust_lang;
mod typescript;

pub use go::GoParser;
pub use python::PythonParser;
pub use rust_lang::RustParser;
pub use typescript::TypeScriptParser;

use crate::dependency_graph::{DependencyGraph, EdgeKind, FileNode, GraphLanguage};
use std::path::{Path, PathBuf};

pub trait LanguageParser {
    fn language(&self) -> GraphLanguage;
    fn parse_file(&self, path: &Path, workspace: &Path) -> Option<FileNode>;
    fn resolve_import(&self, raw: &str, from_file: &Path, workspace: &Path) -> Option<PathBuf>;
}

pub fn build_graph_for_files(
    files: &[PathBuf],
    workspace: &Path,
    language: GraphLanguage,
) -> DependencyGraph {
    let parser: Box<dyn LanguageParser> = match language {
        GraphLanguage::Python => Box::new(PythonParser),
        GraphLanguage::TypeScript => Box::new(TypeScriptParser),
        GraphLanguage::Go => Box::new(GoParser),
        GraphLanguage::Rust => Box::new(RustParser),
        GraphLanguage::Unknown => return DependencyGraph::new(),
    };

    let mut graph = DependencyGraph::new();

    for file in files {
        if let Some(node) = parser.parse_file(file, workspace) {
            let imports = node.imports.clone();
            let node_path = node.path.clone();
            graph.add_node(node);

            for import_ref in &imports {
                if let Some(resolved) =
                    parser.resolve_import(&import_ref.raw, &node_path, workspace)
                {
                    graph.add_edge(node_path.clone(), resolved, EdgeKind::Import);
                }
            }
        }
    }

    graph
}

pub fn canonicalize_workspace_path(path: &Path, workspace: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    }
}
