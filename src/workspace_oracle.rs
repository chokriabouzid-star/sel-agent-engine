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

    /// Read the root package's test script without interpreting shell syntax.
    fn package_json_test_script(&self) -> Option<String> {
        let raw = std::fs::read_to_string(self.workspace.join("package.json")).ok()?;
        let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let script = json.get("scripts")?.get("test")?.as_str()?.trim();
        if script.is_empty() {
            None
        } else {
            Some(script.to_string())
        }
    }

    ///            LLM
    pub fn resolve_test_command(&self, target: &str) -> (String, Vec<String>) {
        let t = target.to_lowercase();
        let p_type = self.current_type();

        match p_type {
            ProjectType::Go => {
                // Phase 3b: honour an explicit `go test ...` invocation from the caller.
                match caller_test_extras(target, |tool| tool == "go" || tool.ends_with("/go")) {
                    Some(extras) => ("go".to_string(), go_args_from_extras(extras)),
                    None => (
                        "go".to_string(),
                        vec!["test".to_string(), "./...".to_string(), "-v".to_string()],
                    ),
                }
            }
            ProjectType::Rust => {
                // Phase 3b: honour an explicit `cargo test ...` invocation from the caller.
                match caller_test_extras(target, |tool| tool == "cargo" || tool.ends_with("/cargo"))
                {
                    Some(extras) => ("cargo".to_string(), cargo_args_from_extras(extras)),
                    None => (
                        "cargo".to_string(),
                        vec![
                            "test".to_string(),
                            "--".to_string(),
                            "--nocapture".to_string(),
                        ],
                    ),
                }
            }
            ProjectType::Node => {
                // Let npm execute the declared script and supply its environment.
                // Do not inject Jest-only flags into an arbitrary test runner.
                if self.package_json_test_script().is_some() {
                    ("npm".to_string(), vec!["test".to_string()])
                } else if t.contains("jest")
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

    // Protect Go package patterns and ellipses before splitting on sentence periods.
    const GO_ALL_PLACEHOLDER: &str = "__SEL_GO_ALL_PACKAGES__";
    const ELLIPSIS_PLACEHOLDER: &str = "__SEL_ELLIPSIS__";
    let protected = lc
        .replace("./...", GO_ALL_PLACEHOLDER)
        .replace("...", ELLIPSIS_PLACEHOLDER);

    let mut clauses = Vec::new();
    let mut clause_start = 0usize;
    for (index, ch) in protected.char_indices() {
        let next_index = index + ch.len_utf8();
        let sentence_period = ch == '.'
            && protected
                .get(next_index..)
                .and_then(|tail| tail.chars().next())
                .map(|next| next.is_whitespace())
                .unwrap_or(true);

        if matches!(ch, ';' | '\n' | '!' | '?') || sentence_period {
            clauses.push(protected[clause_start..index].to_owned());
            clause_start = next_index;
        }
    }
    clauses.push(protected[clause_start..].to_owned());

    for clause in clauses {
        let clause = clause
            .replace(GO_ALL_PLACEHOLDER, "./...")
            .replace(ELLIPSIS_PLACEHOLDER, "...");
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
#[allow(clippy::items_after_test_module)]
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

// ---------------------------------------------------------------------------
// Phase 3b: caller-flag preservation helpers (pure, no I/O).
// ---------------------------------------------------------------------------

/// Characters that mean "this target is a shell pipeline, not a bare argv".
const SHELL_METACHARS: [char; 9] = ['&', '|', ';', '>', '<', '$', '`', '\'', '"'];

/// Go flags that consume the NEXT token as their value (so it is not a package).
const GO_VALUE_FLAGS: [&str; 8] = [
    "-run",
    "-bench",
    "-timeout",
    "-count",
    "-parallel",
    "-tags",
    "-benchtime",
    "-cpu",
];

/// Returns the tokens AFTER `<tool> test` when `target` is a plain, safe
/// `<tool> test <something>` invocation. Returns `None` (-> caller keeps its
/// hard-coded default) for bare targets, non-`test` subcommands and anything
/// containing shell metacharacters.
fn caller_test_extras(target: &str, tool_matches: fn(&str) -> bool) -> Option<Vec<String>> {
    if target.chars().any(|c| SHELL_METACHARS.contains(&c)) {
        return None;
    }
    let toks: Vec<&str> = target.split_whitespace().collect();
    if toks.len() < 3 || !tool_matches(toks[0]) || toks[1] != "test" {
        return None;
    }
    Some(toks[2..].iter().map(|s| s.to_string()).collect())
}

/// Builds `go test ...` args, guaranteeing `args[0] == "test"` (the `-race`
/// injection in runner.rs inserts at index 1), a package scope, and `-v`
/// (parse_go_tests counts `--- PASS:` lines, which only exist with `-v`).
fn go_args_from_extras(extras: Vec<String>) -> Vec<String> {
    let mut args = vec!["test".to_string()];
    let mut has_pkg = false;
    let mut expect_value = false;

    for tok in extras {
        if expect_value {
            expect_value = false;
            args.push(tok);
            continue;
        }
        if tok.starts_with('-') {
            if GO_VALUE_FLAGS.contains(&tok.as_str()) {
                expect_value = true;
            }
            args.push(tok);
            continue;
        }
        has_pkg = true;
        args.push(tok);
    }

    if !has_pkg {
        args.push("./...".to_string());
    }
    if !args.iter().any(|a| a == "-v") {
        args.push("-v".to_string());
    }
    args
}

/// Builds `cargo test ...` args, appending the `-- --nocapture` tail when absent.
fn cargo_args_from_extras(extras: Vec<String>) -> Vec<String> {
    let mut args = vec!["test".to_string()];
    args.extend(extras);
    if !args.iter().any(|a| a == "--") {
        args.push("--".to_string());
    }
    if !args.iter().any(|a| a == "--nocapture") {
        args.push("--nocapture".to_string());
    }
    args
}

#[cfg(test)]
mod race_clause_split_tests {
    //! W1: a negation in a separate sentence (split by '.') must not
    //! suppress an explicit -race request in another sentence.
    use super::goal_requires_go_race;

    #[test]
    fn bug_dot_separated_negation_suppresses_race() {
        assert!(goal_requires_go_race(
            "Fix the data race. go test -race ./... must pass. Do not modify the test files."
        ));
    }

    #[test]
    fn bug_concurrent_phrasing_with_dot_negation() {
        assert!(goal_requires_go_race(
            "It must be safe for concurrent use, and go test -race ./... must pass. Do not modify counter_test.go."
        ));
    }

    #[test]
    fn bug_go_path_dots_followed_by_space() {
        assert!(goal_requires_go_race(
            "Run go test -race ./... and make it pass. Do not touch tests."
        ));
    }

    #[test]
    fn guard_newline_separated_still_true() {
        assert!(goal_requires_go_race(
            "go test -race ./... must pass.\nDo not modify the test files."
        ));
    }

    #[test]
    fn guard_negated_race_detector_still_false() {
        assert!(!goal_requires_go_race(
            "Fix the bug. Do not enable the race detector. Run go test."
        ));
    }

    #[test]
    fn guard_do_not_use_race_still_false() {
        assert!(!goal_requires_go_race("Do not use -race here."));
    }
}
