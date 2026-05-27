// src/repair_strategy.rs  v8.1: Directed Repair + Escalating Strategy
//
// Provides context-aware repair prompts that escalate based on attempt number
// and loop detection. Separates source files from test files to prevent the
// common anti-pattern of "fixing" tests instead of source code.

use std::path::Path;

/// Supported source file extensions.
const SUPPORTED_EXTENSIONS: &[&str] = &["ts", "js", "py", "go", "rs"];

/// Accumulated context for a repair session.
pub struct RepairCtx {
    pub source_files: Vec<String>,
    pub test_files: Vec<String>,
    pub source_file: String,
    pub function_name: String,
    pub prev_errors: Vec<String>,
}

impl RepairCtx {
    /// Build a new repair context by scanning `workspace` for source and test files.
    ///
    /// # Arguments
    /// * `workspace`  directory to scan for project files
    /// * `goal`       the high-level task description (used to extract function name)
    /// * `prev_error`  the previous error message, if any
    pub fn build(workspace: &Path, goal: &str, error_history: &[String]) -> Self {
        let mut source_files = Vec::new();
        let mut test_files = Vec::new();

        if let Ok(entries) = std::fs::read_dir(workspace) {
            for entry in entries.flatten() {
                let ft = match entry.file_type() {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };
                if !ft.is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let ext = Path::new(&name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");

                if !SUPPORTED_EXTENSIONS.contains(&ext) {
                    continue;
                }

                if Self::is_test_file(&name) {
                    test_files.push(name);
                } else {
                    source_files.push(name);
                }
            }
        }

        // Sort for deterministic output
        source_files.sort();
        test_files.sort();

        let source_file = source_files
            .first()
            .cloned()
            .unwrap_or_else(|| "main".to_string());

        let function_name = Self::extract_function_name(goal);

        let prev_errors = error_history.to_vec();

        Self {
            source_files,
            test_files,
            source_file,
            function_name,
            prev_errors,
        }
    }

    /// Determine whether a filename represents a test file.
    fn is_test_file(name: &str) -> bool {
        // Go convention: *_test.go
        if name.ends_with("_test.go") {
            return true;
        }
        // TypeScript / JavaScript conventions
        if name.ends_with(".spec.ts")
            || name.ends_with(".spec.js")
            || name.ends_with(".test.ts")
            || name.ends_with(".test.js")
        {
            return true;
        }
        // Python convention: test_*.py or *_test.py
        if name.starts_with("test_") && name.ends_with(".py") {
            return true;
        }
        if name.ends_with("_test.py") {
            return true;
        }
        // Rust convention: file in tests/ already handled by dir; also catch inline
        if name.contains("test") {
            return true;
        }
        false
    }

    /// Extract a likely function name from the goal description.
    fn extract_function_name(goal: &str) -> String {
        let keywords = ["Fix", "fix", "implement", "Implement", "repair", "Repair"];
        let tokens: Vec<&str> = goal.split_whitespace().collect();
        for &kw in &keywords {
            if let Some(idx) = tokens.iter().position(|&t| t == kw) {
                if idx + 1 < tokens.len() {
                    let raw = tokens[idx + 1];
                    // Strip common punctuation wrappers
                    let cleaned = raw
                        .trim_matches(|c: char| c == '(' || c == ')' || c == '`' || c == '\'' || c == '"');
                    if !cleaned.is_empty() {
                        return cleaned.to_string();
                    }
                }
            }
        }
        "the function".to_string()
    }
}

/// Maximum repair attempts before giving up.
pub const MAX_REPAIR_ATTEMPTS: u8 = 5;

/// Build an escalating repair prompt.
///
/// Prompt severity increases with each attempt, and when a loop is detected
/// (same error repeated), the strategy skips to a more aggressive approach.
pub fn build_prompt(attempt: u8, error: &str, ctx: &RepairCtx) -> String {
    if attempt > MAX_REPAIR_ATTEMPTS {
        return format!(
            "GIVING UP after {} attempts. Last error:\n{}",
            attempt, error
        );
    }

    let is_loop = detect_error_loop(ctx, attempt);

    match (attempt, is_loop) {
        (1, _) => format!(
            "Fix SOURCE FILES only: [{}]\n\
             NEVER touch test files: [{}]\n\
             Error:\n{}",
            ctx.source_files.join(", "),
            ctx.test_files.join(", "),
            error
        ),
        (2, false) => format!(
            "Repair attempt 2. Focus on {}, function `{}`.\n\
             Error:\n{}",
            ctx.source_file, ctx.function_name, error
        ),
        (2, true) => format!(
            "SAME ERROR REPEATED  stop patching tests.\n\
             Which exact line in {} is wrong? Fix ONLY that line.\n\
             Error:\n{}",
            ctx.source_file, error
        ),
        (_, true) => format!(
            "ALL patches failed. REWRITE `{}` from scratch.\n\
             Implement `{}` correctly. Don't copy the broken version.\n\
             Error:\n{}",
            ctx.source_file, ctx.function_name, error
        ),
        (_, false) => format!(
            "Repair attempt {}. Carefully read the error and fix the root cause.\n\
             Error:\n{}",
            attempt, error
        ),
    }
}

/// Detect whether the repair session is stuck in a loop.
fn detect_error_loop(ctx: &RepairCtx, attempt: u8) -> bool {
    // v8.4: Exact consecutive duplicates
    if ctx.prev_errors.windows(2).any(|w| w[0] == w[1]) {
        return true;
    }
    // v8.4: Same error class — first 120 chars match
    if ctx.prev_errors.len() >= 2 {
        let n = ctx.prev_errors.len();
        let a = &ctx.prev_errors[n - 1][..ctx.prev_errors[n - 1].len().min(120)];
        let b = &ctx.prev_errors[n - 2][..ctx.prev_errors[n - 2].len().min(120)];
        if a == b {
            return true;
        }
    }
    // v8.4: Cycling — same error class seen earlier (not just last pair)
    if ctx.prev_errors.len() >= 3 {
        let last = &ctx.prev_errors[ctx.prev_errors.len() - 1];
        let prefix_len = last.len().min(120);
        let cycling = ctx.prev_errors[..ctx.prev_errors.len() - 1]
            .iter()
            .any(|e| e[..e.len().min(120)] == last[..prefix_len]);
        if cycling {
            return true;
        }
    }
    // Heuristic: past attempt 3 without progress
    if attempt > 3 && !ctx.prev_errors.is_empty() {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_test_file_go() {
        assert!(RepairCtx::is_test_file("calc_test.go"));
        assert!(!RepairCtx::is_test_file("calc.go"));
    }

    #[test]
    fn test_is_test_file_ts() {
        assert!(RepairCtx::is_test_file("app.spec.ts"));
        assert!(RepairCtx::is_test_file("app.test.ts"));
        assert!(!RepairCtx::is_test_file("app.ts"));
    }

    #[test]
    fn test_is_test_file_python() {
        assert!(RepairCtx::is_test_file("test_calc.py"));
        assert!(RepairCtx::is_test_file("calc_test.py"));
        assert!(!RepairCtx::is_test_file("calc.py"));
    }

    #[test]
    fn test_extract_function_name_basic() {
        assert_eq!(RepairCtx::extract_function_name("Fix add() function"), "add");
        assert_eq!(RepairCtx::extract_function_name("implement `sort` method"), "sort");
    }

    #[test]
    fn test_extract_function_name_fallback() {
        assert_eq!(RepairCtx::extract_function_name("do something"), "the function");
    }

    #[test]
    fn test_build_prompt_attempt_1() {
        let ctx = RepairCtx {
            source_files: vec!["main.go".into()],
            test_files: vec!["main_test.go".into()],
            source_file: "main.go".into(),
            function_name: "Add".into(),
            prev_errors: vec![],
        };
        let prompt = build_prompt(1, "undefined: Add", &ctx);
        assert!(prompt.contains("Fix SOURCE FILES only"));
        assert!(prompt.contains("NEVER touch test files"));
    }

    #[test]
    fn test_build_prompt_loop_detected() {
        let ctx = RepairCtx {
            source_files: vec!["main.go".into()],
            test_files: vec![],
            source_file: "main.go".into(),
            function_name: "Add".into(),
            prev_errors: vec!["same error".into(), "same error".into()],
        };
        let prompt = build_prompt(2, "same error", &ctx);
        assert!(prompt.contains("SAME ERROR REPEATED"));
    }

    #[test]
    fn test_build_prompt_rewrite_on_persistent_loop() {
        let ctx = RepairCtx {
            source_files: vec!["lib.rs".into()],
            test_files: vec![],
            source_file: "lib.rs".into(),
            function_name: "parse".into(),
            prev_errors: vec!["err".into(), "err".into()],
        };
        let prompt = build_prompt(4, "err", &ctx);
        assert!(prompt.contains("REWRITE"));
    }

    #[test]
    fn test_max_attempts_gives_up() {
        let ctx = RepairCtx {
            source_files: vec![],
            test_files: vec![],
            source_file: "main".into(),
            function_name: "f".into(),
            prev_errors: vec![],
        };
        let prompt = build_prompt(MAX_REPAIR_ATTEMPTS + 1, "fatal", &ctx);
        assert!(prompt.contains("GIVING UP"));
    }

    #[test]
    fn test_detect_error_loop_consecutive_duplicates() {
        let ctx = RepairCtx {
            source_files: vec![],
            test_files: vec![],
            source_file: "main".into(),
            function_name: "f".into(),
            prev_errors: vec!["a".into(), "a".into()],
        };
        assert!(detect_error_loop(&ctx, 2));
    }

    #[test]
    fn test_detect_error_loop_no_duplicates() {
        let ctx = RepairCtx {
            source_files: vec![],
            test_files: vec![],
            source_file: "main".into(),
            function_name: "f".into(),
            prev_errors: vec!["a".into(), "b".into()],
        };
        assert!(!detect_error_loop(&ctx, 2));
    }

    #[test]
    fn test_build_with_empty_workspace() {
        let dir = PathBuf::from("/nonexistent_dir_12345");
        let ctx = RepairCtx::build(&dir, "Fix something", &[]);
        assert!(ctx.source_files.is_empty());
        assert!(ctx.test_files.is_empty());
        assert_eq!(ctx.source_file, "main");
    }
}
