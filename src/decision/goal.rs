// src/decision/goal.rs
//
// Goal validation and goal clarity analysis.
// Extracted from src/decision.rs during v9.2.5 refactor.
// This module is behavior-preserving: logic is unchanged.

/// Validates that the goal is processable.
/// Hard-fail only when the goal is too short to be meaningful.
/// All other quality checks are advisory and should not block execution.
pub fn validate_goal(goal: &str) -> Option<String> {
    let len = goal.trim().len();
    if len < 10 {
        return Some("Goal too short - please describe what you want to achieve.".to_string());
    }
    None
}

/// Advisory quality hints for vague goals.
/// These hints enrich planning, but must never block execution.
pub fn goal_advisory_hints(goal: &str) -> Vec<String> {
    let g = goal.to_lowercase();
    let mut hints = Vec::new();

    let real_keywords = [
        "fix",
        "implement",
        "refactor",
        "update",
        "migrate",
        "failing",
        "crate",
        "existing",
        "workspace",
        "create",
        "write",
        "build",
        "add",
        "remove",
        "change",
        "correct",
        "debug",
        "repair",
        "resolve",
        "make",
        "convert",
    ];

    if real_keywords.iter().any(|kw| g.contains(kw)) {
        return hints;
    }

    let has_test = g.contains("test")
        || g.contains("pytest")
        || g.contains("assert")
        || g.contains("spec")
        || g.contains("verify");

    if !has_test {
        hints.push(
            "HINT: The goal does not mention how success will be verified. Consider specifying which tests or behaviors should pass."
                .to_string(),
        );
    }

    let vague = (g.contains("test") || g.contains("assert"))
        && (g.contains("some value") || g.contains("correct value"));

    if vague {
        hints.push(
            "HINT: The goal uses vague expected values ('some value', 'correct value'). Prefer exact expected values when possible."
                .to_string(),
        );
    }

    hints
}

#[derive(Debug, Clone, PartialEq)]
pub struct GoalClarity {
    pub score: f32,
    pub has_file_mention: bool,
    pub has_test_mention: bool,
    pub has_behavior_spec: bool,
}

impl GoalClarity {
    pub fn analyze(goal: &str) -> Self {
        let trimmed = goal.trim();
        let g = trimmed.to_lowercase();

        let has_file_mention = [
            ".rs",
            ".py",
            ".go",
            ".ts",
            ".js",
            "cargo.toml",
            "package.json",
            "go.mod",
            "src/",
            "tests/",
            "test_",
            "_test.",
        ]
        .iter()
        .any(|kw| g.contains(kw))
            || trimmed.split_whitespace().any(|w| w.contains('/'));

        let has_test_mention = [
            "test",
            "tests",
            "pytest",
            "assert",
            "spec",
            "cargo test",
            "go test",
            "npm test",
            "jest",
            "bench",
        ]
        .iter()
        .any(|kw| g.contains(kw));

        let has_behavior_spec = [
            "should",
            "must",
            "expected",
            "return",
            "returns",
            "panic",
            "error",
            "fail",
            "failing",
            "fix",
            "implement",
            "refactor",
            "handle",
            "support",
        ]
        .iter()
        .any(|kw| g.contains(kw));

        let mut score = 0.0f32;
        if has_file_mention {
            score += 0.35;
        }
        if has_test_mention {
            score += 0.30;
        }
        if has_behavior_spec {
            score += 0.25;
        }
        if trimmed.len() >= 24 {
            score += 0.10;
        }

        Self {
            score: score.min(1.0),
            has_file_mention,
            has_test_mention,
            has_behavior_spec,
        }
    }

    pub fn is_ambiguous(&self) -> bool {
        self.score <= 0.35
    }

    pub fn planning_hint(&self) -> &'static str {
        if self.is_ambiguous() {
            "Before planning: inspect workspace files to understand the project structure and identify the most likely implementation and test files."
        } else {
            ""
        }
    }
}
