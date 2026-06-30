// goal_parser.rs — goal -> ParsedGoal
// Scaffold selection from natural-language goal text.

use crate::scaffold_engine::ProjectKind;

#[derive(Debug, Clone, PartialEq)]
pub enum SubKind {
    // Python
    Flask,
    FastAPI,
    Django,
    // TypeScript/Node
    Express,
    React,
    // None
    Plain,
}

#[derive(Debug, Clone)]
pub struct ParsedGoal {
    pub kind: ProjectKind,
    #[allow(dead_code)]
    pub sub_kind: SubKind,
    pub extra_deps: Vec<String>,
}

pub fn parse(workspace: &std::path::Path, goal: &str) -> ParsedGoal {
    let kind = detect_kind(workspace, goal);
    let sub_kind = detect_sub_kind(&kind, goal);
    let extra_deps = detect_extra_deps(&kind, &sub_kind, goal);

    ParsedGoal {
        kind,
        sub_kind,
        extra_deps,
    }
}

/// Match a standalone token, not an arbitrary substring.
/// Prevents false positives like:
/// - "expression" -> "express"
/// - "reactive"   -> "react"
fn contains_goal_token(goal: &str, needle: &str) -> bool {
    goal.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .any(|token| token == needle)
}

fn contains_any_goal_token(goal: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .copied()
        .any(|needle| contains_goal_token(goal, needle))
}

/// Returns true if the goal explicitly negates a technology.
///
/// Covers all languages and frameworks, not just Python.
/// Examples that return true:
///   "DO NOT use TypeScript"
///   "don't use Express"
///   "avoid React"
///   "without Rust"
///   "no Node.js"
/// Sentence boundary: dot followed by space/end-of-string, or \n, or ;
/// A dot inside a name like "Node.js" is NOT a boundary.
fn find_sentence_end(s: &str) -> usize {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'\n' | b';' => return i,
            b'.' if i + 1 >= len || bytes[i + 1] == b' ' || bytes[i + 1] == b'\n' => {
                // Sentence-ending dot: at end-of-string or followed by whitespace
                // Mid-name dot (e.g. "Node.js") falls through to _ => {}
                return i;
            }
            _ => {}
        }
        i += 1;
    }
    len
}

/// Check if `needle` appears as a contiguous word-sequence in `haystack`.
/// Handles multi-word tech names like ["node","js"] from "node.js".
fn contains_word_sequence(haystack: &[&str], needle: &[&str]) -> bool {
    if needle.is_empty() {
        return false;
    }
    if needle.len() == 1 {
        return haystack.contains(&needle[0]);
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn has_explicit_negation(goal: &str, tech: &str) -> bool {
    let g = goal.to_lowercase();
    let t = tech.to_lowercase();

    // Pattern 1 — direct: "do not use rust"
    let direct = [
        format!("do not use {}", t),
        format!("don't use {}", t),
        format!("dont use {}", t),
        format!("no {}", t),
        format!("not {}", t),
        format!("avoid {}", t),
        format!("without {}", t),
        format!("never use {}", t),
        format!("instead of {}", t),
    ];
    if direct.iter().any(|p| g.contains(p.as_str())) {
        return true;
    }

    // Pattern 2 — list: "do not use TypeScript, Python, or Node.js"
    // Normalize tech to words so "node.js" → ["node","js"]
    let t_words: Vec<&str> = t
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();

    const LIST_STARTERS: &[&str] = &[
        "do not use ",
        "don't use ",
        "dont use ",
        "never use ",
        "avoid ",
        "without ",
    ];

    for starter in LIST_STARTERS {
        let mut search_from = 0usize;
        while let Some(rel_pos) = g[search_from..].find(starter) {
            let abs_pos = search_from + rel_pos;
            let after_starter = &g[abs_pos + starter.len()..];

            let clause_end = find_sentence_end(after_starter);
            let clause = &after_starter[..clause_end];

            let clause_words: Vec<&str> = clause
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|s| !s.is_empty())
                .collect();

            if contains_word_sequence(&clause_words, &t_words) {
                return true;
            }

            search_from = abs_pos + starter.len();
            if search_from >= g.len() {
                break;
            }
        }
    }

    false
}

fn detect_kind(workspace: &std::path::Path, goal: &str) -> ProjectKind {
    if workspace.join("Cargo.toml").exists() {
        return ProjectKind::Rust;
    }
    if workspace.join("go.mod").exists() {
        return ProjectKind::Go;
    }
    if workspace.join("package.json").exists() {
        return ProjectKind::TypeScript;
    }
    if workspace.join("requirements.txt").exists() || workspace.join("pyproject.toml").exists() {
        return ProjectKind::Python;
    }

    let g = goal.to_lowercase();

    // Python is checked FIRST — it is more explicit than TypeScript.
    // If the goal mentions Flask/pytest AND negates TypeScript, Python wins.
    if !has_explicit_negation(&g, "python")
        && (g.contains("fast api")
            || contains_any_goal_token(&g, &["python", "pytest", "flask", "fastapi", "django"]))
    {
        return ProjectKind::Python;
    }

    // TypeScript / Node — only if NOT explicitly negated
    let ts_signals = (contains_goal_token(&g, "typescript")
        && !has_explicit_negation(&g, "typescript"))
        || (contains_goal_token(&g, "ts") && !has_explicit_negation(&g, "typescript"))
        || g.contains(".ts")
        || (contains_goal_token(&g, "express") && !has_explicit_negation(&g, "express"))
        || (contains_goal_token(&g, "react") && !has_explicit_negation(&g, "react"))
        || (contains_goal_token(&g, "jest") && !has_explicit_negation(&g, "jest"));

    if ts_signals {
        return ProjectKind::TypeScript;
    }

    // Rust — only if NOT explicitly negated
    if !has_explicit_negation(&g, "rust")
        && !has_explicit_negation(&g, "cargo")
        && contains_any_goal_token(&g, &["rust", "cargo"])
    {
        return ProjectKind::Rust;
    }

    // Go — conservative (the English verb "go" is too ambiguous)
    if !has_explicit_negation(&g, "golang")
        && (g.contains(" go ") || contains_any_goal_token(&g, &["golang", "gorilla"]))
    {
        return ProjectKind::Go;
    }

    ProjectKind::Unknown
}

fn detect_sub_kind(kind: &ProjectKind, goal: &str) -> SubKind {
    let g = goal.to_lowercase();

    match kind {
        ProjectKind::Python => {
            if g.contains("fast api") || contains_goal_token(&g, "fastapi") {
                SubKind::FastAPI
            } else if contains_goal_token(&g, "flask") {
                SubKind::Flask
            } else if contains_goal_token(&g, "django") {
                SubKind::Django
            } else {
                SubKind::Plain
            }
        }
        ProjectKind::TypeScript => {
            if contains_goal_token(&g, "express") && !has_explicit_negation(&g, "express") {
                SubKind::Express
            } else if contains_goal_token(&g, "react") && !has_explicit_negation(&g, "react") {
                SubKind::React
            } else {
                SubKind::Plain
            }
        }
        _ => SubKind::Plain,
    }
}

fn detect_extra_deps(kind: &ProjectKind, sub_kind: &SubKind, goal: &str) -> Vec<String> {
    let g = goal.to_lowercase();

    // v7.5 fix: bypass extra_deps extraction for QuickFix tests
    if g.contains("do not use pip_install")
        || g.contains("do not use pip install")
        || g.contains("strict rule")
    {
        return vec![];
    }

    let mut deps: Vec<String> = vec![];

    match kind {
        ProjectKind::Python => match sub_kind {
            SubKind::FastAPI => {
                deps.push("fastapi".into());
                deps.push("uvicorn[standard]".into());
                deps.push("httpx".into()); // TestClient
            }
            SubKind::Flask => {
                deps.push("flask".into());
            }
            SubKind::Django => {
                deps.push("django".into());
                deps.push("pytest-django".into());
            }
            SubKind::Plain => {
                if contains_goal_token(&g, "requests") {
                    deps.push("requests".into());
                }
                if contains_goal_token(&g, "sqlalchemy") {
                    deps.push("sqlalchemy".into());
                }
                if contains_goal_token(&g, "pydantic") {
                    deps.push("pydantic".into());
                }
            }
            _ => {}
        },
        ProjectKind::TypeScript => {
            match sub_kind {
                SubKind::Express => {
                    deps.push("express".into());
                    deps.push("@types/express".into());
                    deps.push("supertest".into());
                    deps.push("@types/supertest".into());
                }
                SubKind::React => {
                    deps.push("react".into());
                    deps.push("react-dom".into());
                    deps.push("@types/react".into());
                }
                _ => {}
            }

            if contains_goal_token(&g, "axios") {
                deps.push("axios".into());
            }

            if contains_goal_token(&g, "supertest") && !deps.contains(&"supertest".to_string()) {
                deps.push("supertest".into());
                deps.push("@types/supertest".into());
            }
        }
        _ => {}
    }

    deps
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fake_ws() -> &'static Path {
        Path::new("/tmp")
    }

    #[test]
    fn test_fastapi_detection() {
        let g = parse(
            fake_ws(),
            "Create a Python FastAPI application with /hello route",
        );
        assert_eq!(g.kind, ProjectKind::Python);
        assert_eq!(g.sub_kind, SubKind::FastAPI);
        assert!(g.extra_deps.contains(&"fastapi".to_string()));
        assert!(g.extra_deps.contains(&"uvicorn[standard]".to_string()));
    }

    #[test]
    fn test_flask_detection() {
        let g = parse(
            fake_ws(),
            "Create a Flask app with a /hello route and pytest tests",
        );
        assert_eq!(g.kind, ProjectKind::Python);
        assert_eq!(g.sub_kind, SubKind::Flask);
        assert!(g.extra_deps.contains(&"flask".to_string()));
    }

    #[test]
    fn test_express_detection() {
        let g = parse(
            fake_ws(),
            "Create a TypeScript Express API with /status endpoint",
        );
        assert_eq!(g.kind, ProjectKind::TypeScript);
        assert_eq!(g.sub_kind, SubKind::Express);
        assert!(g.extra_deps.contains(&"express".to_string()));
    }

    #[test]
    fn test_plain_python() {
        let g = parse(fake_ws(), "Create a Python calculator with pytest tests");
        assert_eq!(g.kind, ProjectKind::Python);
        assert_eq!(g.sub_kind, SubKind::Plain);
        assert!(g.extra_deps.is_empty());
    }

    #[test]
    fn test_expression_does_not_trigger_express_detection() {
        let g = parse(
            fake_ws(),
            "Create a Python Flask web calculator in app.py with a single page '/' showing a calculator UI. Clicking = sends POST to '/calculate' with JSON {'expression': '...'}. The server returns JSON {'result': ...}. Write test_calc.py using Flask test client and pytest.",
        );
        assert_eq!(g.kind, ProjectKind::Python);
        assert_eq!(g.sub_kind, SubKind::Flask);
        assert!(g.extra_deps.contains(&"flask".to_string()));
        assert!(!g.extra_deps.contains(&"express".to_string()));
    }

    #[test]
    fn test_reactive_does_not_trigger_react_detection() {
        let g = parse(
            fake_ws(),
            "Create a TypeScript reactive event pipeline with Jest tests",
        );
        assert_eq!(g.kind, ProjectKind::TypeScript);
        assert_eq!(g.sub_kind, SubKind::Plain);
        assert!(!g.extra_deps.contains(&"react".to_string()));
    }

    #[test]
    fn test_rust_no_deps() {
        let g = parse(fake_ws(), "Create a Rust function that adds two numbers");
        assert_eq!(g.kind, ProjectKind::Rust);
        assert_eq!(g.sub_kind, SubKind::Plain);
        assert!(g.extra_deps.is_empty());
    }

    // ── Negation tests — apply to ALL languages ──────────────────────────

    #[test]
    fn test_negated_typescript_resolves_to_python() {
        let g = parse(
            fake_ws(),
            "Create a Python Flask web calculator. DO NOT use Node.js, TypeScript, or Express. Use ONLY Python and Flask. Run pytest.",
        );
        assert_eq!(g.kind, ProjectKind::Python);
        assert_eq!(g.sub_kind, SubKind::Flask);
        assert!(g.extra_deps.contains(&"flask".to_string()));
        assert!(!g.extra_deps.contains(&"express".to_string()));
    }

    #[test]
    fn test_negated_express_resolves_to_python() {
        let g = parse(
            fake_ws(),
            "Build a Flask REST API. Do not use Express or Node.js. Python only.",
        );
        assert_eq!(g.kind, ProjectKind::Python);
        assert_eq!(g.sub_kind, SubKind::Flask);
    }

    #[test]
    fn test_do_not_use_typescript_with_only_python_phrase() {
        let g = parse(
            fake_ws(),
            "Write a pytest test suite for a Flask app. Use ONLY Python and Flask. DO NOT use Node.js, TypeScript, or Express.",
        );
        assert_eq!(g.kind, ProjectKind::Python);
    }

    #[test]
    fn test_explicit_typescript_positive_still_works() {
        let g = parse(
            fake_ws(),
            "Create a TypeScript Express API with /status endpoint and Jest tests",
        );
        assert_eq!(g.kind, ProjectKind::TypeScript);
        assert_eq!(g.sub_kind, SubKind::Express);
    }

    #[test]
    fn test_negated_react_does_not_trigger_react_scaffold() {
        let g = parse(
            fake_ws(),
            "Build a TypeScript REST API. Do not use React. Use Jest for testing.",
        );
        assert_eq!(g.kind, ProjectKind::TypeScript);
        assert_eq!(g.sub_kind, SubKind::Plain);
        assert!(!g.extra_deps.contains(&"react".to_string()));
    }
    // ── list-negation tests (v9.3.5) ────────────────────────────────

    #[test]
    fn negation_list_first_item_detected() {
        assert!(has_explicit_negation(
            "DO NOT use TypeScript, Python, or Go",
            "typescript"
        ));
    }

    #[test]
    fn negation_list_middle_item_detected() {
        assert!(has_explicit_negation(
            "DO NOT use TypeScript, Python, or Go",
            "python"
        ));
    }

    #[test]
    fn negation_list_last_item_detected() {
        assert!(has_explicit_negation(
            "DO NOT use TypeScript, Python, or Go",
            "go"
        ));
    }

    #[test]
    fn negation_list_nodejs_detected() {
        // "Node.js" contains a dot — must not break clause boundary detection
        assert!(has_explicit_negation(
            "DO NOT use Python, Flask, TypeScript, Go, or Node.js.",
            "node.js"
        ));
    }

    #[test]
    fn negation_list_nodejs_as_node_detected() {
        assert!(has_explicit_negation(
            "DO NOT use Python, Flask, TypeScript, Go, or Node.js.",
            "node"
        ));
    }

    #[test]
    fn non_negated_tech_not_affected_by_list() {
        // "rust" does not appear in the negation list
        assert!(!has_explicit_negation(
            "DO NOT use TypeScript, Python, or Go. Use ONLY Rust.",
            "rust"
        ));
    }

    #[test]
    fn dont_use_list_also_works() {
        assert!(has_explicit_negation(
            "don't use React, TypeScript, or Express",
            "react"
        ));
    }

    #[test]
    fn sentence_end_dot_does_not_bleed_into_next_sentence() {
        // "Go" is negated in first sentence; should not affect second sentence
        assert!(has_explicit_negation("DO NOT use Go. Use ONLY Rust.", "go"));
        assert!(!has_explicit_negation(
            "DO NOT use Go. Use ONLY Rust.",
            "rust"
        ));
    }

    #[test]
    fn detect_rust_goal_with_negation_list() {
        let workspace = std::path::Path::new("/tmp/__nonexistent_sel_test__");
        let goal = "Build a calculator in Rust.             DO NOT use TypeScript, Python, or Go.             Use ONLY Rust. Run cargo test.";
        let parsed = parse(workspace, goal);
        assert_eq!(parsed.kind, ProjectKind::Rust);
    }

    #[test]
    fn detect_go_goal_with_negation_list() {
        let workspace = std::path::Path::new("/tmp/__nonexistent_sel_test__");
        let goal = "Build a Go API with net/http.             DO NOT use Python, Flask, TypeScript.             Use ONLY Go. Run go test.";
        let parsed = parse(workspace, goal);
        assert_eq!(parsed.kind, ProjectKind::Go);
    }
}
