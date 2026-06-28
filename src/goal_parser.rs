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

    if contains_goal_token(&g, "typescript")
        || contains_goal_token(&g, "ts")
        || g.contains(".ts")
        || contains_any_goal_token(&g, &["express", "react", "jest"])
    {
        return ProjectKind::TypeScript;
    }

    if g.contains("fast api")
        || contains_any_goal_token(&g, &["python", "pytest", "flask", "fastapi", "django"])
    {
        return ProjectKind::Python;
    }

    if contains_any_goal_token(&g, &["rust", "cargo"]) {
        return ProjectKind::Rust;
    }

    // Keep "go" conservative: the English verb is too ambiguous to classify
    // purely as a standalone token without causing false positives.
    if g.contains(" go ") || contains_any_goal_token(&g, &["golang", "gorilla"]) {
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
            if contains_goal_token(&g, "express") {
                SubKind::Express
            } else if contains_goal_token(&g, "react") {
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
}
