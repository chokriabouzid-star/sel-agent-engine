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

        for entry in walkdir::WalkDir::new(workspace)
            .max_depth(3)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            if !SUPPORTED_EXTENSIONS.contains(&ext) {
                continue;
            }

            // skip venv / node_modules / target / .git
            let path_str = path.to_string_lossy();
            if path_str.contains("/venv/")
                || path_str.contains("/node_modules/")
                || path_str.contains("/target/")
                || path_str.contains("/.git/")
            {
                continue;
            }

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if name.is_empty() {
                continue;
            }

            if Self::is_test_file(&name) {
                test_files.push(name);
            } else {
                source_files.push(name);
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
        // Rust convention: specific suffixes only — avoid false positives
        if name.ends_with("_test.rs") || name.ends_with("_tests.rs") {
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
                    let cleaned = raw.trim_matches(|c: char| {
                        c == '(' || c == ')' || c == '`' || c == '\'' || c == '"'
                    });
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

fn truncate_pattern_example(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

fn build_pattern_hint(matched_pattern: Option<&crate::pattern_library::Pattern>) -> String {
    match matched_pattern {
        Some(pattern) => {
            let mut lines = vec![format!(
                "Known successful repair pattern: {:?}.",
                pattern.route
            )];

            let hint = pattern.route.hint();
            if !hint.is_empty() {
                lines.push(format!("Guidance: {}", hint));
            }

            if let Some(example_fix) = pattern
                .example_fix
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                lines.push(format!(
                    "Example successful fix: {}",
                    truncate_pattern_example(example_fix, 160)
                ));
            }

            lines.join("\n") + "\n"
        }
        None => String::new(),
    }
}

fn build_route_instruction(route: &crate::pattern_library::RepairRoute) -> String {
    match route {
        crate::pattern_library::RepairRoute::Generic => String::new(),
        crate::pattern_library::RepairRoute::ForceSourceOnly => {
            "REPAIR ROUTE: ForceSourceOnly\nHARD CONSTRAINT: Modify implementation files only. Test files are immutable. Restore behavior by editing source files, not tests.\n".to_string()
        }
        crate::pattern_library::RepairRoute::MissingDependency => {
            "REPAIR ROUTE: MissingDependency\nFocus first on missing imports, missing modules, and dependency declarations. Prefer the smallest dependency/import fix before changing logic.\n".to_string()
        }
        crate::pattern_library::RepairRoute::FunctionDeleted => {
            "REPAIR ROUTE: FunctionDeleted\nA required function or symbol is missing or renamed. Restore the expected symbol and contract before broader refactors.\n".to_string()
        }
        crate::pattern_library::RepairRoute::NullGuard => {
            "REPAIR ROUTE: NullGuard\nAdd a nil/null/None guard before the failing access, then keep existing behavior unchanged.\n".to_string()
        }
        crate::pattern_library::RepairRoute::RustOwnership => {
            "REPAIR ROUTE: RustOwnership\nFix the borrow or ownership conflict with references, clone(), lifetimes, or by reducing overlapping borrows.\n".to_string()
        }
        crate::pattern_library::RepairRoute::CircularImport => {
            "REPAIR ROUTE: CircularImport\nBreak the import cycle with a smaller dependency boundary, lazy import, or interface extraction. Do not duplicate business logic.\n".to_string()
        }
        crate::pattern_library::RepairRoute::TypeMismatch => {
            "REPAIR ROUTE: TypeMismatch\nAlign actual and expected types. Check function signatures, return types, and call-site arguments before changing behavior.\n".to_string()
        }
    }
}


/// Build an escalating repair prompt.
///
/// Prompt severity increases with each attempt, and when a loop is detected
/// (same error repeated), the strategy skips to a more aggressive approach.
pub fn build_prompt(
    attempt: u8,
    error: &str,
    ctx: &RepairCtx,
    matched_pattern: Option<&crate::pattern_library::Pattern>,
    effective_route: &crate::pattern_library::RepairRoute,
) -> String {
    let pattern_hint = build_pattern_hint(matched_pattern);
    let route_instruction = build_route_instruction(effective_route);

    if attempt > MAX_REPAIR_ATTEMPTS {
        return format!(
            "GIVING UP after {} attempts.\n{}{}Last error:\n{}",
            attempt,
            route_instruction,
            pattern_hint,
            error
        );
    }

    if error.contains("CONSTITUTION_VIOLATION:no-modify-tests") {
        return format!(
            "CRITICAL CONSTRAINT VIOLATION.\n             You attempted to modify a protected test file.\n             NEVER write or patch any test file: [{}].\n             Fix SOURCE files ONLY: [{}].\n             The tests define the contract and are immutable.\n             {}{}Read the error carefully and change only implementation files.\n             Error:\n{}",
            ctx.test_files.join(", "),
            ctx.source_files.join(", "),
            route_instruction,
            pattern_hint,
            error
        );
    }

    let is_loop = detect_error_loop(ctx, attempt);

    match (attempt, is_loop) {
        (1, _) => format!(
            "Fix SOURCE FILES only: [{}]\n\
             NEVER touch test files: [{}]\n\
             {}{}Error:\n{}",
            ctx.source_files.join(", "),
            ctx.test_files.join(", "),
            route_instruction,
            pattern_hint,
            error
        ),
        (2, false) => format!(
            "Repair attempt 2. Focus on {}, function `{}`.\n\
             {}{}Error:\n{}",
            ctx.source_file,
            ctx.function_name,
            route_instruction,
            pattern_hint,
            error
        ),
        (2, true) => format!(
            "SAME ERROR REPEATED  stop patching tests.\n\
             Which exact line in {} is wrong? Fix ONLY that line.\n\
             {}{}Error:\n{}",
            ctx.source_file,
            route_instruction,
            pattern_hint,
            error
        ),
        (_, true) => format!(
            "ALL patches failed. REWRITE `{}` from scratch.\n\
             Implement `{}` correctly. Don't copy the broken version.\n\
             {}{}Error:\n{}",
            ctx.source_file,
            ctx.function_name,
            route_instruction,
            pattern_hint,
            error
        ),
        (_, false) => format!(
            "Repair attempt {}. Carefully read the error and fix the root cause.\n\
             {}{}Error:\n{}",
            attempt,
            route_instruction,
            pattern_hint,
            error
        ),
    }
}

/// Detect whether the repair session is stuck in a loop.
/// Safe UTF-8 prefix: never panics on multi-byte characters.
fn safe_prefix(s: &str, max_bytes: usize) -> &str {
    if max_bytes >= s.len() {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn detect_error_loop(ctx: &RepairCtx, attempt: u8) -> bool {
    // v8.4: Exact consecutive duplicates
    if ctx.prev_errors.windows(2).any(|w| w[0] == w[1]) {
        return true;
    }
    // v8.4: Same error class — first 120 chars match
    if ctx.prev_errors.len() >= 2 {
        let n = ctx.prev_errors.len();
        let a = safe_prefix(&ctx.prev_errors[n - 1], 120);
        let b = safe_prefix(&ctx.prev_errors[n - 2], 120);
        if a == b {
            return true;
        }
    }
    // v8.4: Cycling — same error class seen earlier (not just last pair)
    if ctx.prev_errors.len() >= 3 {
        let last = &ctx.prev_errors[ctx.prev_errors.len() - 1];
        let last_prefix = safe_prefix(last, 120);
        let cycling = ctx.prev_errors[..ctx.prev_errors.len() - 1]
            .iter()
            .any(|e| safe_prefix(e, 120) == last_prefix);
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
        assert_eq!(
            RepairCtx::extract_function_name("Fix add() function"),
            "add"
        );
        assert_eq!(
            RepairCtx::extract_function_name("implement `sort` method"),
            "sort"
        );
    }

    #[test]
    fn test_extract_function_name_fallback() {
        assert_eq!(
            RepairCtx::extract_function_name("do something"),
            "the function"
        );
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
        let prompt = build_prompt(1, "undefined: Add", &ctx, None, &crate::pattern_library::RepairRoute::Generic);
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
        let prompt = build_prompt(2, "same error", &ctx, None, &crate::pattern_library::RepairRoute::Generic);
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
        let prompt = build_prompt(4, "err", &ctx, None, &crate::pattern_library::RepairRoute::Generic);
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
        let prompt = build_prompt(MAX_REPAIR_ATTEMPTS + 1, "fatal", &ctx, None, &crate::pattern_library::RepairRoute::Generic);
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
    fn test_build_prompt_includes_pattern_hint() {
        let ctx = RepairCtx {
            source_files: vec!["main.py".into()],
            test_files: vec!["test_main.py".into()],
            source_file: "main.py".into(),
            function_name: "load".into(),
            prev_errors: vec![],
        };
        let pattern = crate::pattern_library::Pattern {
            id: "python:abc".into(),
            language: "python".into(),
            error_signature: "ImportError".into(),
            route: crate::pattern_library::RepairRoute::CircularImport,
            success_count: 2,
            failure_count: 0,
            usage_count: 2,
            last_seen_utc: "2026-01-01T00:00:00Z".into(),
            failed_contexts: vec![],
            example_fix: None,
        };
        let prompt = build_prompt(1, "ImportError", &ctx, Some(&pattern), &pattern.route);
        assert!(prompt.contains("Known successful repair pattern"));
        assert!(prompt.contains("CircularImport"));
        assert!(prompt.contains("Guidance:"));
    }

    #[test]
    fn test_build_prompt_includes_example_fix() {
        let ctx = RepairCtx {
            source_files: vec!["src/apiClient.ts".into()],
            test_files: vec!["src/apiClient.test.ts".into()],
            source_file: "src/apiClient.ts".into(),
            function_name: "fetchWithRetry".into(),
            prev_errors: vec![],
        };
        let pattern = crate::pattern_library::Pattern {
            id: "typescript:def".into(),
            language: "typescript".into(),
            error_signature: "Cannot find module".into(),
            route: crate::pattern_library::RepairRoute::MissingDependency,
            success_count: 3,
            failure_count: 0,
            usage_count: 3,
            last_seen_utc: "2026-01-01T00:00:00Z".into(),
            failed_contexts: vec![],
            example_fix: Some(
                "patch_file:src/apiClient.ts | run_tests:npm test".into(),
            ),
        };
        let prompt = build_prompt(1, "Cannot find module", &ctx, Some(&pattern), &pattern.route);
        assert!(prompt.contains("Example successful fix:"));
        assert!(prompt.contains("patch_file:src/apiClient.ts"));
    }

    #[test]
    fn test_build_prompt_includes_effective_route_without_pattern() {
        let ctx = RepairCtx {
            source_files: vec!["main.py".into()],
            test_files: vec!["test_main.py".into()],
            source_file: "main.py".into(),
            function_name: "load".into(),
            prev_errors: vec![],
        };
        let prompt = build_prompt(
            1,
            "ModuleNotFoundError: No module named 'requests'",
            &ctx,
            None,
            &crate::pattern_library::RepairRoute::MissingDependency,
        );
        assert!(prompt.contains("REPAIR ROUTE: MissingDependency"));
        assert!(prompt.contains("Focus first on missing imports"));
    }

    #[test]
    fn test_truncate_pattern_example_adds_ellipsis() {
        let long = "x".repeat(200);
        let out = truncate_pattern_example(&long, 20);
        assert_eq!(out.chars().count(), 21);
        assert!(out.ends_with('…'));
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
