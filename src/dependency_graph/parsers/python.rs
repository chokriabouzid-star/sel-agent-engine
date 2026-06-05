use super::LanguageParser;
use crate::dependency_graph::{FileNode, GraphLanguage, ImportRef};
use std::path::{Path, PathBuf};

pub struct PythonParser;

impl LanguageParser for PythonParser {
    fn language(&self) -> GraphLanguage {
        GraphLanguage::Python
    }

    fn parse_file(&self, path: &Path, _workspace: &Path) -> Option<FileNode> {
        let content = std::fs::read_to_string(path).ok()?;
        let mut node = FileNode::new(path, GraphLanguage::Python);

        for line in content.lines() {
            let line = line.trim();

            // Skip comments
            if line.starts_with('#') {
                continue;
            }

            // `import x` or `import x.y`
            if let Some(rest) = line.strip_prefix("import ") {
                let module = rest
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(',');
                if is_local_python_module(module) {
                    node.imports.push(ImportRef::new(module, vec![]));
                }
            }

            // `from x import y` or `from .x import y`
            if let Some(rest) = line.strip_prefix("from ") {
                if let Some(import_pos) = rest.find(" import ") {
                    let module = rest[..import_pos].trim();
                    let symbols_str = rest[import_pos + 8..].trim();
                    let symbols: Vec<String> = symbols_str
                        .trim_matches(|c| c == '(' || c == ')')
                        .split(',')
                        .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
                        .filter(|s| !s.is_empty())
                        .collect();

                    if is_local_python_module(module) {
                        node.imports.push(ImportRef::new(module, symbols));
                    }
                }
            }
        }

        // exports = public top-level defs
        for line in content.lines() {
            let line = line.trim();
            if let Some(name) = extract_python_export(line) {
                node.exports.push(name);
            }
        }

        Some(node)
    }

    fn resolve_import(&self, raw: &str, from_file: &Path, workspace: &Path) -> Option<PathBuf> {
        let from_dir = from_file.parent()?;

        // Relative: .foo or ..foo
        if raw.starts_with('.') {
            let dots = raw.chars().take_while(|c| *c == '.').count();
            let rest = &raw[dots..];
            let module_parts: Vec<&str> = rest.split('.').collect();

            let mut base = from_dir.to_path_buf();
            for _ in 1..dots {
                base = base.parent()?.to_path_buf();
            }

            if module_parts.is_empty() || rest.is_empty() {
                // from . import x — package __init__.py
                let candidate = base.join("__init__.py");
                if candidate.exists() {
                    return Some(candidate);
                }
                return None;
            }

            // Try module_parts joined
            let rel: PathBuf = module_parts.iter().collect();
            let as_module = base.join(&rel).with_extension("py");
            if as_module.exists() {
                return Some(as_module);
            }
            let as_pkg = base.join(&rel).join("__init__.py");
            if as_pkg.exists() {
                return Some(as_pkg);
            }

            return None;
        }

        // Absolute local (only if exists within workspace)
        let parts: Vec<&str> = raw.split('.').collect();
        let rel: PathBuf = parts.iter().collect();

        let as_module = workspace.join(&rel).with_extension("py");
        if as_module.exists() {
            return Some(as_module);
        }
        let as_pkg = workspace.join(&rel).join("__init__.py");
        if as_pkg.exists() {
            return Some(as_pkg);
        }

        None
    }
}

fn is_local_python_module(module: &str) -> bool {
    // Relative imports always local
    if module.starts_with('.') {
        return true;
    }
    // Skip known stdlib / well-known packages
    let stdlib: &[&str] = &[
        "os",
        "sys",
        "re",
        "io",
        "abc",
        "ast",
        "csv",
        "copy",
        "math",
        "json",
        "time",
        "uuid",
        "enum",
        "typing",
        "types",
        "pathlib",
        "logging",
        "hashlib",
        "datetime",
        "functools",
        "itertools",
        "collections",
        "contextlib",
        "dataclasses",
        "unittest",
        "subprocess",
        "threading",
        "multiprocessing",
        "concurrent",
        "asyncio",
        "socket",
        "http",
        "urllib",
        "email",
        "html",
        "xml",
        "sqlite3",
        "struct",
        "string",
        "random",
        "secrets",
        "base64",
        "binascii",
        "codecs",
        "traceback",
        "inspect",
        "warnings",
        "weakref",
        "gc",
        "builtins",
        "pytest",
        "flask",
        "fastapi",
        "django",
        "sqlalchemy",
        "requests",
        "numpy",
        "pandas",
        "scipy",
        "matplotlib",
        "sklearn",
        "torch",
        "tensorflow",
        "pydantic",
        "celery",
        "redis",
        "boto3",
        "aiohttp",
        "httpx",
        "click",
        "typer",
        "rich",
        "attrs",
        "marshmallow",
    ];

    let root = module.split('.').next().unwrap_or(module);
    !stdlib.contains(&root)
}

fn extract_python_export(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("def ") {
        let name = rest.split('(').next()?.trim().to_string();
        if !name.starts_with('_') {
            return Some(name);
        }
    }
    if let Some(rest) = line.strip_prefix("class ") {
        let name = rest.split(['(', ':']).next()?.trim().to_string();
        if !name.starts_with('_') {
            return Some(name);
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
    fn parses_absolute_import() {
        let dir = setup(&[
            ("main.py", "import utils\n"),
            ("utils.py", "def helper(): pass\n"),
        ]);
        let parser = PythonParser;
        let node = parser
            .parse_file(&dir.path().join("main.py"), dir.path())
            .unwrap();
        assert_eq!(node.imports.len(), 1);
        assert_eq!(node.imports[0].raw, "utils");
    }

    #[test]
    fn parses_from_import() {
        let dir = setup(&[("main.py", "from utils import foo, bar\n")]);
        let parser = PythonParser;
        let node = parser
            .parse_file(&dir.path().join("main.py"), dir.path())
            .unwrap();
        assert_eq!(node.imports[0].raw, "utils");
        assert_eq!(node.imports[0].symbols, vec!["foo", "bar"]);
    }

    #[test]
    fn ignores_stdlib() {
        let dir = setup(&[("main.py", "import os\nimport sys\nimport utils\n")]);
        let parser = PythonParser;
        let node = parser
            .parse_file(&dir.path().join("main.py"), dir.path())
            .unwrap();
        assert_eq!(node.imports.len(), 1);
        assert_eq!(node.imports[0].raw, "utils");
    }

    #[test]
    fn resolves_relative_import() {
        let dir = setup(&[
            ("pkg/main.py", "from .utils import foo\n"),
            ("pkg/utils.py", "def foo(): pass\n"),
        ]);
        let parser = PythonParser;
        let resolved = parser.resolve_import(".utils", &dir.path().join("pkg/main.py"), dir.path());
        assert_eq!(resolved, Some(dir.path().join("pkg/utils.py")));
    }

    #[test]
    fn resolves_absolute_workspace_import() {
        let dir = setup(&[("main.py", "import utils\n"), ("utils.py", "")]);
        let parser = PythonParser;
        let resolved = parser.resolve_import("utils", &dir.path().join("main.py"), dir.path());
        assert_eq!(resolved, Some(dir.path().join("utils.py")));
    }

    #[test]
    fn exports_public_defs() {
        let dir = setup(&[(
            "lib.py",
            "def foo(): pass\ndef _private(): pass\nclass Bar: pass\n",
        )]);
        let parser = PythonParser;
        let node = parser
            .parse_file(&dir.path().join("lib.py"), dir.path())
            .unwrap();
        assert!(node.exports.contains(&"foo".to_string()));
        assert!(node.exports.contains(&"Bar".to_string()));
        assert!(!node.exports.contains(&"_private".to_string()));
    }
}
