//! Diagnostic engine  analyzes errors and produces actionable hints.
//!
//! Takes raw compiler/test error output and maps it to structured hints
//! that guide the repair strategy and the LLM prompt builder.

use std::fmt;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Severity level of a diagnostic hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Fatal,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Severity::Info    => "INFO",
            Severity::Warning => "WARN",
            Severity::Error   => "ERROR",
            Severity::Fatal   => "FATAL",
        };
        write!(f, "{}", s)
    }
}

/// A single actionable hint derived from error output.
#[derive(Debug, Clone)]
pub struct Hint {
    pub severity: Severity,
    pub category: &'static str,
    pub message: String,
    pub suggestion: String,
}

impl fmt::Display for Hint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}][{}] {}  {}",
            self.severity, self.category, self.message, self.suggestion
        )
    }
}

/// Complete diagnostic report for a build/test failure.
#[derive(Debug, Default)]
pub struct DiagnosticReport {
    pub hints: Vec<Hint>,
}

impl DiagnosticReport {
    pub fn is_empty(&self) -> bool {
        self.hints.is_empty()
    }

    /// Return the highest severity found.
    pub fn max_severity(&self) -> Option<Severity> {
        self.hints.iter().map(|h| h.severity).max()
    }

    /// Format all hints as a compact bullet list for inclusion in LLM prompts.
    pub fn as_prompt_fragment(&self) -> String {
        self.hints
            .iter()
            .map(|h| format!(" [{}] {}  {}", h.category, h.message, h.suggestion))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ---------------------------------------------------------------------------
// Analyzer
// ---------------------------------------------------------------------------

/// Analyze raw error text and produce a `DiagnosticReport`.
pub fn analyze(error_text: &str) -> DiagnosticReport {
    let mut hints = Vec::new();

    // --- Go patterns ---
    if error_text.contains("undefined:") {
        let sym = extract_after(error_text, "undefined:", 40);
        hints.push(Hint {
            severity: Severity::Error,
            category: "go/undefined",
            message: format!("undefined symbol: {}", sym.trim()),
            suggestion: "Add the missing import or define the symbol before use".into(),
        });
    }
    if error_text.contains("declared but not used") {
        hints.push(Hint {
            severity: Severity::Error,
            category: "go/unused-var",
            message: "variable declared but not used".into(),
            suggestion: "Remove the unused variable or use `_` to discard it".into(),
        });
    }
    if error_text.contains("imported and not used") {
        hints.push(Hint {
            severity: Severity::Error,
            category: "go/unused-import",
            message: "package imported but not used".into(),
            suggestion: "Remove the unused import statement".into(),
        });
    }
    if error_text.contains("cannot use") && error_text.contains("as type") {
        hints.push(Hint {
            severity: Severity::Error,
            category: "go/type-mismatch",
            message: "type mismatch in assignment or argument".into(),
            suggestion: "Check the expected type and convert or cast accordingly".into(),
        });
    }

    // --- Rust patterns ---
    if error_text.contains("cannot borrow") && error_text.contains("as mutable") {
        hints.push(Hint {
            severity: Severity::Error,
            category: "rust/borrow",
            message: "cannot borrow as mutable".into(),
            suggestion: "Use `mut` binding or restructure ownership".into(),
        });
    }
    if error_text.contains("use of moved value") {
        hints.push(Hint {
            severity: Severity::Error,
            category: "rust/move",
            message: "use of moved value".into(),
            suggestion: "Clone the value before moving, or use references".into(),
        });
    }
    if error_text.contains("mismatched types") {
        hints.push(Hint {
            severity: Severity::Error,
            category: "rust/type",
            message: "mismatched types".into(),
            suggestion: "Verify return type, function signature, or add explicit type conversion".into(),
        });
    }
    if error_text.contains("unused import") || error_text.contains("unused variable") {
        hints.push(Hint {
            severity: Severity::Warning,
            category: "rust/unused",
            message: "unused import or variable (warning as error)".into(),
            suggestion: "Remove or prefix with `_` to suppress".into(),
        });
    }

    // --- Python patterns ---
    if error_text.contains("SyntaxError") {
        let detail = extract_after(error_text, "SyntaxError:", 60);
        hints.push(Hint {
            severity: Severity::Error,
            category: "python/syntax",
            message: format!("SyntaxError: {}", detail.trim()),
            suggestion: "Check parentheses, indentation, and colon placement".into(),
        });
    }
    if error_text.contains("IndentationError") {
        hints.push(Hint {
            severity: Severity::Error,
            category: "python/indent",
            message: "IndentationError".into(),
            suggestion: "Use consistent 4-space indentation; do not mix tabs and spaces".into(),
        });
    }
    if error_text.contains("ModuleNotFoundError") || error_text.contains("ImportError") {
        let module = extract_after(error_text, "No module named", 40);
        hints.push(Hint {
            severity: Severity::Error,
            category: "python/import",
            message: format!("missing module:{}", module.trim()),
            suggestion: "Add the module as a dependency or fix the import path".into(),
        });
    }

    // --- Python dataclass mutable default / field() misuse ---
    if error_text.contains("Field' object has no attribute")
        || error_text.contains("Field object has no attribute")
        || error_text.contains("mutable default")
        || error_text.contains("default_factory")
    {
        hints.push(Hint {
            severity: Severity::Error,
            category: "python/dataclass-default",
            message: "dataclass field uses mutable default or field() incorrectly".into(),
            suggestion: "Keep @dataclass, import `field` from dataclasses, and replace `items: list = []` with `items: list = field(default_factory=list)`".into(),
        });
    }

    if (error_text.contains("SyntaxError") && error_text.contains("dataclass"))
        || error_text.contains("dataclass class")
    {
        hints.push(Hint {
            severity: Severity::Error,
            category: "python/dataclass-syntax",
            message: "invalid dataclass syntax".into(),
            suggestion: "Use `@dataclass` on its own line directly above `class Name:`; do not write `dataclass class ...`".into(),
        });
    }

    // --- TypeScript patterns ---
    if error_text.contains("TS2304") {
        let sym = extract_after(error_text, "TS2304:", 50);
        hints.push(Hint {
            severity: Severity::Error,
            category: "ts/undefined",
            message: format!("TS2304 cannot find name:{}", sym.trim()),
            suggestion: "Import the symbol or declare it in the appropriate scope".into(),
        });
    }
    if error_text.contains("TS2322") {
        hints.push(Hint {
            severity: Severity::Error,
            category: "ts/type-mismatch",
            message: "TS2322 type mismatch".into(),
            suggestion: "Correct the type annotation or use a type assertion".into(),
        });
    }

    // --- Generic / test runner patterns ---
    if error_text.contains("FAIL") && error_text.contains("panic") {
        hints.push(Hint {
            severity: Severity::Fatal,
            category: "test/panic",
            message: "test panicked".into(),
            suggestion: "Check index bounds, unwrap calls, and slice operations".into(),
        });
    }
    if error_text.contains("timeout") || error_text.contains("timed out") {
        hints.push(Hint {
            severity: Severity::Fatal,
            category: "test/timeout",
            message: "test timed out".into(),
            suggestion: "Check for infinite loops, blocking I/O, or deadlocks".into(),
        });
    }

    // Fallback  no specific pattern matched
    if hints.is_empty() {
        hints.push(Hint {
            severity: Severity::Error,
            category: "generic",
            message: "unrecognized error pattern".into(),
            suggestion: "Read the full error carefully and fix the root cause".into(),
        });
    }

    DiagnosticReport { hints }
}

/// Extract a short snippet of text following `after` in `text`.
fn extract_after(text: &str, after: &str, max_len: usize) -> String {
    if let Some(pos) = text.find(after) {
        let start = pos + after.len();
        let end = (start + max_len).min(text.len());
        // Stop at newline
        let snippet = &text[start..end];
        if let Some(nl) = snippet.find('\n') {
            return snippet[..nl].to_string();
        }
        return snippet.to_string();
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_go_undefined_detected() {
        let err = "broken.go:4:2: undefined: fmt";
        let report = analyze(err);
        assert!(!report.is_empty());
        let categories: Vec<_> = report.hints.iter().map(|h| h.category).collect();
        assert!(categories.contains(&"go/undefined"));
    }

    #[test]
    fn test_rust_borrow_detected() {
        let err = "error[E0596]: cannot borrow `data` as mutable, as it is not declared as mutable";
        let report = analyze(err);
        let cats: Vec<_> = report.hints.iter().map(|h| h.category).collect();
        assert!(cats.contains(&"rust/borrow"));
    }

    #[test]
    fn test_python_syntax_detected() {
        let err = "  File \"broken.py\", line 1\nSyntaxError: unexpected EOF";
        let report = analyze(err);
        let cats: Vec<_> = report.hints.iter().map(|h| h.category).collect();
        assert!(cats.contains(&"python/syntax"));
    }

    #[test]
    fn test_timeout_detected() {
        let err = "--- FAIL: TestAdd (30.00s)\npanic: test timed out after 30s";
        let report = analyze(err);
        let severities: Vec<_> = report.hints.iter().map(|h| h.severity).collect();
        assert!(severities.contains(&Severity::Fatal));
    }

    #[test]
    fn test_fallback_hint_on_unknown() {
        let report = analyze("something went wrong, dunno why");
        assert_eq!(report.hints[0].category, "generic");
    }

    #[test]
    fn test_max_severity() {
        let report = analyze("--- FAIL: TestAdd (30.00s)\npanic: test timed out");
        assert_eq!(report.max_severity(), Some(Severity::Fatal));
    }

    #[test]
    fn test_as_prompt_fragment_nonempty() {
        let report = analyze("undefined: fmt");
        let fragment = report.as_prompt_fragment();
        assert!(fragment.contains(""));
        assert!(fragment.contains("go/undefined"));
    }

    #[test]
    fn test_go_unused_import() {
        let err = r#"./main.go:3:8: "fmt" imported and not used"#;
        let report = analyze(err);
        let cats: Vec<_> = report.hints.iter().map(|h| h.category).collect();
        assert!(cats.contains(&"go/unused-import"));
    }

    #[test]
    fn test_python_dataclass_default_detected() {
        let err = "AttributeError: 'Field' object has no attribute 'append'";
        let report = analyze(err);
        let cats: Vec<_> = report.hints.iter().map(|h| h.category).collect();
        assert!(cats.contains(&"python/dataclass-default"));
    }

    #[test]
    fn test_python_dataclass_syntax_detected() {
        let err = "SyntaxError: invalid syntax\n dataclass class Item:";
        let report = analyze(err);
        let cats: Vec<_> = report.hints.iter().map(|h| h.category).collect();
        assert!(cats.contains(&"python/dataclass-syntax"));
    }
}
