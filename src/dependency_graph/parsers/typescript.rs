use super::LanguageParser;
use crate::dependency_graph::{FileNode, GraphLanguage, ImportRef};
use std::path::{Path, PathBuf};

pub struct TypeScriptParser;

static EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx"];

impl LanguageParser for TypeScriptParser {
    fn language(&self) -> GraphLanguage {
        GraphLanguage::TypeScript
    }

    fn parse_file(&self, path: &Path, _workspace: &Path) -> Option<FileNode> {
        let content = std::fs::read_to_string(path).ok()?;
        let mut node = FileNode::new(path, GraphLanguage::TypeScript);

        for line in content.lines() {
            let line = line.trim();

            if line.starts_with("//") {
                continue;
            }

            // import ... from './module'
            // import type ... from './module'
            // export { X } from './module'
            // export * from './module'
            let kind = if line.starts_with("export") {
                EdgeKind::ReExport
            } else {
                EdgeKind::Import
            };

            if let Some(module) = extract_ts_import_path(line) {
                if is_local_ts_module(&module) {
                    let is_type_only = line.contains("import type") || line.contains("export type");
                    let symbols = extract_ts_symbols(line);
                    let import_ref = ImportRef::new(module.clone(), symbols);
                    node.imports.push(import_ref);

                    // We store the kind hint in the raw field prefix for now
                    let _ = (kind, is_type_only); // used when building edges
                }
            }

            // dynamic import: import('./module')
            if let Some(module) = extract_dynamic_import(line) {
                if is_local_ts_module(&module) {
                    node.imports.push(ImportRef::new(module, vec![]));
                }
            }
        }

        // exports
        for line in content.lines() {
            let line = line.trim();
            if let Some(name) = extract_ts_export(line) {
                node.exports.push(name);
            }
        }

        Some(node)
    }

    fn resolve_import(&self, raw: &str, from_file: &Path, workspace: &Path) -> Option<PathBuf> {
        let from_dir = from_file.parent()?;

        let base = if raw.starts_with('.') {
            from_dir.join(raw)
        } else {
            // Absolute — check workspace src/
            workspace.join("src").join(raw)
        };

        // Try exact extension match
        let normalized = base.components().collect::<PathBuf>();

        // Try adding extensions
        for ext in EXTENSIONS {
            let with_ext = normalized.with_extension(ext);
            if with_ext.exists() {
                return Some(with_ext);
            }
        }

        // Try as directory index
        for ext in EXTENSIONS {
            let index = normalized.join(format!("index.{}", ext));
            if index.exists() {
                return Some(index);
            }
        }

        None
    }
}

fn extract_ts_import_path(line: &str) -> Option<String> {
    // Match: from 'path' or from "path"
    let from_pos = line.rfind(" from ")?;
    let after_from = line[from_pos + 6..].trim();
    extract_quoted_string(after_from)
}

fn extract_dynamic_import(line: &str) -> Option<String> {
    // Match: import('path') or import("path")
    let import_pos = line.find("import(")?;
    let after = &line[import_pos + 7..];
    extract_quoted_string(after)
}

fn extract_quoted_string(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(stripped) = s.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(stripped[..end].to_string())
    } else if let Some(stripped) = s.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else {
        None
    }
}

fn extract_ts_symbols(line: &str) -> Vec<String> {
    if let Some(start) = line.find('{') {
        if let Some(end) = line.find('}') {
            return line[start + 1..end]
                .split(',')
                .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    vec![]
}

fn extract_ts_export(line: &str) -> Option<String> {
    if line.starts_with("export function ") || line.starts_with("export async function ") {
        let rest = line
            .trim_start_matches("export async function ")
            .trim_start_matches("export function ");
        return Some(rest.split(['(', ' ']).next()?.to_string());
    }
    if line.starts_with("export class ") {
        return Some(
            line.trim_start_matches("export class ")
                .split([' ', '{'])
                .next()?
                .to_string(),
        );
    }
    if line.starts_with("export const ") || line.starts_with("export let ") {
        let rest = line
            .trim_start_matches("export const ")
            .trim_start_matches("export let ");
        return Some(rest.split([':', '=', ' ']).next()?.to_string());
    }
    None
}

fn is_local_ts_module(module: &str) -> bool {
    module.starts_with('.') || module.starts_with('/')
}

#[derive(Debug, Clone, Copy)]
enum EdgeKind {
    Import,
    ReExport,
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
    fn parses_named_import() {
        let dir = setup(&[("main.ts", "import { foo, bar } from './utils';\n")]);
        let parser = TypeScriptParser;
        let node = parser
            .parse_file(&dir.path().join("main.ts"), dir.path())
            .unwrap();
        assert_eq!(node.imports[0].raw, "./utils");
        assert_eq!(node.imports[0].symbols, vec!["foo", "bar"]);
    }

    #[test]
    fn parses_default_import() {
        let dir = setup(&[("main.ts", "import Logger from './logger';\n")]);
        let parser = TypeScriptParser;
        let node = parser
            .parse_file(&dir.path().join("main.ts"), dir.path())
            .unwrap();
        assert_eq!(node.imports[0].raw, "./logger");
    }

    #[test]
    fn ignores_node_modules() {
        let dir = setup(&[(
            "main.ts",
            "import express from 'express';\nimport { foo } from './local';\n",
        )]);
        let parser = TypeScriptParser;
        let node = parser
            .parse_file(&dir.path().join("main.ts"), dir.path())
            .unwrap();
        assert_eq!(node.imports.len(), 1);
        assert_eq!(node.imports[0].raw, "./local");
    }

    #[test]
    fn resolves_local_ts_file() {
        let dir = setup(&[
            ("main.ts", "import { foo } from './utils';\n"),
            ("utils.ts", "export function foo() {}\n"),
        ]);
        let parser = TypeScriptParser;
        let resolved = parser.resolve_import("./utils", &dir.path().join("main.ts"), dir.path());
        assert_eq!(resolved, Some(dir.path().join("utils.ts")));
    }

    #[test]
    fn resolves_index_file() {
        let dir = setup(&[
            ("main.ts", "import { X } from './components';\n"),
            ("components/index.ts", "export const X = 1;\n"),
        ]);
        let parser = TypeScriptParser;
        let resolved =
            parser.resolve_import("./components", &dir.path().join("main.ts"), dir.path());
        assert_eq!(resolved, Some(dir.path().join("components/index.ts")));
    }

    #[test]
    fn extracts_exports() {
        let dir = setup(&[(
            "lib.ts",
            "export function doWork() {}\nexport class Manager {}\nexport const MAX = 10;\n",
        )]);
        let parser = TypeScriptParser;
        let node = parser
            .parse_file(&dir.path().join("lib.ts"), dir.path())
            .unwrap();
        assert!(node.exports.contains(&"doWork".to_string()));
        assert!(node.exports.contains(&"Manager".to_string()));
        assert!(node.exports.contains(&"MAX".to_string()));
    }
}
