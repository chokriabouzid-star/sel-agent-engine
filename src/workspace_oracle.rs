use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    Rust,
    Go,
    Node,
    Python,
    Unknown,
}

impl ProjectType {
    #[allow(dead_code)] // utility: ProjectType display — used in diagnostics and logging
    pub fn as_str(&self) -> &str {
        match self {
            ProjectType::Rust => "Rust",
            ProjectType::Go => "Go",
            ProjectType::Node => "Node.js",
            ProjectType::Python => "Python",
            ProjectType::Unknown => "Unknown",
        }
    }
}

pub struct WorkspaceOracle {
    pub workspace: PathBuf,
}

impl WorkspaceOracle {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    pub fn current_type(&self) -> ProjectType {
        Self::detect_project_type(&self.workspace)
    }

    fn detect_project_type(path: &Path) -> ProjectType {
        if let Some(t) = Self::detect_root_markers(path) {
            return t;
        }
        // Nested project: a manifest can live one or two levels down
        // (e.g. `mylib/Cargo.toml` created by `cargo new mylib --lib`).
        if let Some(t) = Self::detect_nested_markers(path, 2) {
            return t;
        }
        ProjectType::Unknown
    }

    /// Root-only detection. Order matters: manifests first, loose sources last.
    fn detect_root_markers(path: &Path) -> Option<ProjectType> {
        if path.join("Cargo.toml").exists() {
            return Some(ProjectType::Rust);
        }
        if path.join("go.mod").exists()
            || std::fs::read_dir(path)
                .map(|dir| {
                    dir.filter_map(Result::ok)
                        .any(|e| e.path().extension().is_some_and(|ext| ext == "go"))
                })
                .unwrap_or(false)
        {
            return Some(ProjectType::Go);
        }
        if path.join("package.json").exists() {
            return Some(ProjectType::Node);
        }
        if path.join("setup.py").exists()
            || path.join("pyproject.toml").exists()
            || path.join("requirements.txt").exists()
            || path.join("venv").exists()
            || std::fs::read_dir(path)
                .map(|dir| {
                    dir.filter_map(Result::ok)
                        .any(|e| e.path().extension().is_some_and(|ext| ext == "py"))
                })
                .unwrap_or(false)
        {
            return Some(ProjectType::Python);
        }
        None
    }

    /// Bounded search for a manifest in child directories.
    fn detect_nested_markers(path: &Path, depth: usize) -> Option<ProjectType> {
        if depth == 0 {
            return None;
        }
        const SKIP: &[&str] = &[
            "target",
            "node_modules",
            "venv",
            ".git",
            ".venv",
            "dist",
            "build",
            "__pycache__",
        ];
        let entries = std::fs::read_dir(path).ok()?;
        let mut dirs: Vec<std::path::PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| !n.starts_with('.') && !SKIP.contains(&n))
                    .unwrap_or(false)
            })
            .collect();
        dirs.sort();

        for dir in &dirs {
            if dir.join("Cargo.toml").exists() {
                return Some(ProjectType::Rust);
            }
            if dir.join("go.mod").exists() {
                return Some(ProjectType::Go);
            }
            if dir.join("package.json").exists() {
                return Some(ProjectType::Node);
            }
            if dir.join("pyproject.toml").exists()
                || dir.join("setup.py").exists()
                || dir.join("requirements.txt").exists()
            {
                return Some(ProjectType::Python);
            }
        }
        for dir in &dirs {
            if let Some(t) = Self::detect_nested_markers(dir, depth - 1) {
                return Some(t);
            }
        }
        None
    }

    /// Checks whether the given file extension is allowed in the detected project type.
    pub fn is_ext_allowed(&self, ext: &str) -> Result<(), String> {
        let p_type = self.current_type();
        let allowed = match p_type {
            ProjectType::Rust => ["rs", "toml", "md"].contains(&ext),
            ProjectType::Go => ["go", "mod", "sum", "sh", "md"].contains(&ext),
            ProjectType::Node => ["js", "ts", "tsx", "json", "md"].contains(&ext),
            ProjectType::Python => ["py", "txt", "cfg", "toml", "ini", "md", "sql"].contains(&ext),
            ProjectType::Unknown => true,
        };

        if allowed {
            Ok(())
        } else {
            Err(format!(
                "LANGUAGE LOCK BLOCKED: Cannot operate on '.{}' file in a {:?} workspace.",
                ext, p_type
            ))
        }
    }

    ///            LLM
    pub fn resolve_test_command(&self, target: &str) -> (String, Vec<String>) {
        let t = target.to_lowercase();
        let p_type = self.current_type();

        match p_type {
            ProjectType::Go => {
                if t.contains("cargo") {
                    return (
                        "go".to_string(),
                        vec!["test".to_string(), "./...".to_string(), "-v".to_string()],
                    );
                }
                (
                    "go".to_string(),
                    vec!["test".to_string(), "./...".to_string(), "-v".to_string()],
                )
            }
            ProjectType::Rust => (
                "cargo".to_string(),
                vec![
                    "test".to_string(),
                    "--".to_string(),
                    "--nocapture".to_string(),
                ],
            ),
            ProjectType::Node => {
                if t.contains("jest")
                    || t.ends_with(".ts")
                    || t.ends_with(".js")
                    || t.contains("npm ")
                    || t.contains("test")
                {
                    (
                        "npx".to_string(),
                        vec![
                            "jest".to_string(),
                            "--runInBand".to_string(),
                            "--forceExit".to_string(),
                        ],
                    )
                } else {
                    (
                        "npm".to_string(),
                        vec![
                            "test".to_string(),
                            "--".to_string(),
                            "--runInBand".to_string(),
                            "--forceExit".to_string(),
                        ],
                    )
                }
            }
            ProjectType::Python => {
                let pytest = if self.workspace.join("venv/bin/pytest").exists() {
                    "venv/bin/pytest"
                } else {
                    "pytest"
                };
                (
                    pytest.to_string(),
                    vec!["-v".to_string(), "--tb=short".to_string()],
                )
            }
            _ => {
                // v7.3.6: Smart split for compound commands in unknown projects
                let parts: Vec<String> = target.split_whitespace().map(|s| s.to_string()).collect();
                if parts.is_empty() {
                    match Self::detect_project_type(&self.workspace) {
                        ProjectType::Go => (
                            "go".to_string(),
                            vec!["test".to_string(), "./...".to_string(), "-v".to_string()],
                        ),
                        ProjectType::Rust => (
                            "cargo".to_string(),
                            vec![
                                "test".to_string(),
                                "--".to_string(),
                                "--nocapture".to_string(),
                            ],
                        ),
                        ProjectType::Node => (
                            "npm".to_string(),
                            vec![
                                "test".to_string(),
                                "--".to_string(),
                                "--runInBand".to_string(),
                                "--forceExit".to_string(),
                            ],
                        ),
                        ProjectType::Python => {
                            let pytest = if self.workspace.join("venv/bin/pytest").exists() {
                                "venv/bin/pytest"
                            } else {
                                "pytest"
                            };
                            (
                                pytest.to_string(),
                                vec!["-v".to_string(), "--tb=short".to_string()],
                            )
                        }
                        ProjectType::Unknown => (target.to_string(), vec![]),
                    }
                } else {
                    let prog = parts[0].clone();
                    let args = parts[1..].to_vec();
                    if prog == "go" && (args.is_empty() || args == vec!["test"]) {
                        (
                            "go".to_string(),
                            vec!["test".to_string(), "./...".to_string(), "-v".to_string()],
                        )
                    } else {
                        (prog, args)
                    }
                }
            }
        }
    }

    ///         LLM
    #[allow(dead_code)] // documented in docs/architecture/oracle.md — planned for plan validation pipeline
    pub fn validate_plan_cmd(&self, cmd: &crate::protocol::Cmd) -> Result<(), String> {
        use crate::protocol::Cmd;
        let p_type = self.current_type();
        match cmd {
            Cmd::Run { command } => {
                let lc = command.to_lowercase();
                match p_type {
                    ProjectType::Rust
                        if lc.contains("go test")
                            || lc.contains("pytest")
                            || lc.contains("npm ") =>
                    {
                        Err("Plan contains non-Rust commands in a Rust project.".to_string())
                    }
                    ProjectType::Go
                        if lc.contains("cargo ")
                            || lc.contains("pytest")
                            || lc.contains("npm ") =>
                    {
                        Err("Plan contains non-Go commands in a Go project.".to_string())
                    }
                    ProjectType::Node
                        if lc.contains("cargo ")
                            || lc.contains("go test")
                            || lc.contains("pytest") =>
                    {
                        Err("Plan contains non-Node commands in a Node.js project.".to_string())
                    }
                    ProjectType::Python
                        if lc.contains("cargo ")
                            || lc.contains("go test")
                            || lc.contains("npm ") =>
                    {
                        Err("Plan contains non-Python commands in a Python project.".to_string())
                    }
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
}

pub fn goal_requires_go_race(goal: &str) -> bool {
    let lc = goal.to_lowercase();
    // Split into clauses so a negation in one sentence cannot be cancelled by
    // an unrelated positive phrase elsewhere in the goal.
    for clause in lc.split([';', '\n', '!', '?']) {
        let c = clause.trim();
        if c.is_empty() {
            continue;
        }
        if !clause_mentions_race(c) {
            continue;
        }
        if clause_negates(c) {
            // Explicit refusal in this clause -> never force the detector.
            return false;
        }
        if clause_requests_race(c) {
            return true;
        }
    }
    false
}

/// True when the clause talks about Go race detection at all.
fn clause_mentions_race(c: &str) -> bool {
    c.contains("race")
}

/// Explicit refusal markers. Conservative on purpose: only clear negations.
fn clause_negates(c: &str) -> bool {
    c.contains("do not")
        || c.contains("don't")
        || c.contains("dont ")
        || c.contains("never")
        || c.contains("without")
        || c.contains("disable")
        || c.contains("skip the race")
        || c.contains("no -race")
        || c.contains("must not")
}

/// Positive request patterns (same surface as the original P0 rule).
fn clause_requests_race(c: &str) -> bool {
    c.contains("go test -race")
        || c.contains("-race ./...")
        || c.contains("-race .")
        || c.contains("race detector")
        || (c.contains("race condition") && (c.contains("must pass") || c.contains("no race")))
        || (c.contains("-race") && c.contains("pass"))
}

#[cfg(test)]
mod p0_goal_race_tests {
    use super::goal_requires_go_race;

    #[test]
    fn p0_race_positive() {
        assert!(goal_requires_go_race("go test -race ./... must pass"));
        assert!(goal_requires_go_race("ensure -race ./... passes"));
        assert!(goal_requires_go_race("race detector must find nothing"));
        assert!(goal_requires_go_race(
            "simulate 1000 concurrent requests — no race conditions — must pass",
        ));
    }

    #[test]
    fn p0_race_negative() {
        assert!(!goal_requires_go_race("go test ./... must pass"));
        assert!(!goal_requires_go_race("run all Go tests"));
        assert!(!goal_requires_go_race("cargo test -- --nocapture"));
        assert!(!goal_requires_go_race(""));
    }
}
