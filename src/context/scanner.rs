// src/context/scanner.rs — v2.0: Centralized Project Scanner
// مسؤول عن اكتشاف اللغة وبنية المشروع

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────
// Types
// ─────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Language {
    Rust,
    Python,
    Go,
    Node,
    TypeScript,
    Unknown,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Rust => write!(f, "Rust"),
            Language::Python => write!(f, "Python"),
            Language::Go => write!(f, "Go"),
            Language::Node => write!(f, "Node.js"),
            Language::TypeScript => write!(f, "TypeScript"),
            Language::Unknown => write!(f, "Unknown"),
        }
    }
}

pub struct Scanner;

impl Scanner {
    pub fn scan(workspace: &Path) -> ProjectProfile {
        let project_name = workspace
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // 1. detect language via manifest
        let (language, dependency_file, base_confidence) = Self::detect_language(workspace);

        // 2. entry points
        let entry_points = Self::detect_entry_points(workspace, &language);
        let entry_bonus: f32 = if !entry_points.is_empty() { 0.2 } else { 0.0 };

        // 3. tests
        let (has_tests, test_framework) = Self::detect_tests(workspace, &language);
        let test_bonus: f32 = if has_tests { 0.2 } else { 0.0 };
        let framework_bonus: f32 = if test_framework.is_some() { 0.1 } else { 0.0 };

        // 4. confidence
        let confidence =
            (base_confidence + entry_bonus + test_bonus + framework_bonus).min(1.0_f32);

        // 5. commands
        let (build_cmd, test_cmd) = Self::infer_commands(&language, &test_framework);

        ProjectProfile {
            project_name,
            language,
            dependency_file,
            entry_points,
            test_framework,
            has_tests,
            build_cmd,
            test_cmd,
            confidence,
        }
    }

    fn detect_language(workspace: &Path) -> (Language, Option<PathBuf>, f32) {
        let manifests: &[(&str, Language, f32)] = &[
            ("Cargo.toml", Language::Rust, 0.5),
            ("go.mod", Language::Go, 0.5),
            ("package.json", Language::Node, 0.5),
            ("tsconfig.json", Language::TypeScript, 0.5),
        ];

        for (filename, lang, base) in manifests {
            let p = workspace.join(filename);
            if p.exists() {
                if *lang == Language::TypeScript {
                    if Self::has_files_with_ext(workspace, "ts") {
                        return (Language::TypeScript, Some(p), *base);
                    }
                    continue;
                }
                return (lang.clone(), Some(p), *base);
            }
        }

        if Self::has_files_with_ext(workspace, "ts") {
            return (Language::TypeScript, None, 0.4);
        }

        let py_manifests = [
            "pyproject.toml",
            "setup.py",
            "setup.cfg",
            "requirements.txt",
        ];
        for name in &py_manifests {
            let p = workspace.join(name);
            if p.exists() {
                return (Language::Python, Some(p), 0.5);
            }
        }

        if Self::has_files_with_ext(workspace, "py") {
            return (Language::Python, None, 0.3);
        }

        (Language::Unknown, None, 0.0)
    }

    fn detect_entry_points(workspace: &Path, language: &Language) -> Vec<PathBuf> {
        let candidates: &[&str] = match language {
            Language::Rust => &["src/main.rs", "src/lib.rs"],
            Language::Python => &["main.py", "app.py", "__main__.py", "src/main.py"],
            Language::Go => &["main.go", "cmd/main.go"],
            Language::Node => &["index.js", "src/index.js", "app.js"],
            Language::TypeScript => &["index.ts", "src/index.ts", "src/main.ts"],
            Language::Unknown => &[],
        };

        candidates
            .iter()
            .map(|c| workspace.join(c))
            .filter(|p| p.exists())
            .collect()
    }

    fn detect_tests(workspace: &Path, language: &Language) -> (bool, Option<String>) {
        match language {
            Language::Rust => {
                let tests_dir = workspace.join("tests");
                let has = tests_dir.exists()
                    || Self::dir_contains_pattern(workspace.join("src"), "#[test]");
                let framework = if has {
                    Some("cargo test".to_string())
                } else {
                    None
                };
                (has, framework)
            }
            Language::Python => {
                let pytest_ini = ["pytest.ini", "pyproject.toml", "setup.cfg"]
                    .iter()
                    .any(|f| workspace.join(f).exists());
                let tests_dir = workspace.join("tests").exists() || workspace.join("test").exists();
                let has_test_files = Self::has_files_matching(workspace, "test_")
                    || Self::has_files_matching(workspace, "_test.py");

                if pytest_ini || tests_dir || has_test_files {
                    (true, Some("pytest".to_string()))
                } else {
                    (false, None)
                }
            }
            Language::Go => {
                let has = Self::has_files_matching(workspace, "_test.go");
                let framework = if has {
                    Some("go test".to_string())
                } else {
                    None
                };
                (has, framework)
            }
            Language::Node | Language::TypeScript => {
                let pkg = workspace.join("package.json");
                if pkg.exists() {
                    if let Ok(content) = fs::read_to_string(&pkg) {
                        if content.contains("\"test\"") {
                            let fw = if content.contains("jest") {
                                "jest"
                            } else if content.contains("mocha") {
                                "mocha"
                            } else {
                                "npm test"
                            };
                            return (true, Some(fw.to_string()));
                        }
                    }
                }
                (false, None)
            }
            Language::Unknown => (false, None),
        }
    }

    fn infer_commands(
        language: &Language,
        test_framework: &Option<String>,
    ) -> (Option<String>, Option<String>) {
        match language {
            Language::Rust => (
                Some("cargo build --release".to_string()),
                Some("cargo test".to_string()),
            ),
            Language::Python => (
                None,
                test_framework
                    .clone()
                    .or_else(|| Some("pytest".to_string())),
            ),
            Language::Go => (
                Some("go build ./...".to_string()),
                Some("go test ./...".to_string()),
            ),
            Language::Node | Language::TypeScript => (
                Some("npm install && npm run build".to_string()),
                Some(
                    test_framework
                        .clone()
                        .unwrap_or_else(|| "npm test".to_string()),
                ),
            ),
            Language::Unknown => (None, None),
        }
    }

    pub fn has_ts_files(workspace: &Path) -> bool {
        Self::has_files_with_ext(workspace, "ts")
    }

    fn has_files_with_ext(dir: &Path, ext: &str) -> bool {
        fs::read_dir(dir).ok().is_some_and(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some(ext))
        })
    }

    fn has_files_matching(dir: &Path, pattern: &str) -> bool {
        Self::walk_dir(dir, 3).iter().any(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains(pattern))
        })
    }

    fn dir_contains_pattern(dir: PathBuf, pattern: &str) -> bool {
        Self::walk_dir(&dir, 2)
            .iter()
            .any(|p| fs::read_to_string(p).is_ok_and(|content| content.contains(pattern)))
    }

    fn walk_dir(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
        let mut results = Vec::new();
        if max_depth == 0 || !dir.is_dir() {
            return results;
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    results.push(path);
                } else if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !matches!(name, "target" | "node_modules" | ".git" | "__pycache__") {
                        results.extend(Self::walk_dir(&path, max_depth - 1));
                    }
                }
            }
        }
        results
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectProfile {
    pub project_name: String,
    pub language: Language,
    pub dependency_file: Option<PathBuf>,
    pub entry_points: Vec<PathBuf>,
    pub test_framework: Option<String>,
    pub has_tests: bool,
    pub build_cmd: Option<String>,
    pub test_cmd: Option<String>,
    pub confidence: f32,
}
