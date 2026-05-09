// src/diagnostic.rs — SPO v2.1: Plugin-based Diagnostic Engine

pub trait DiagnosticRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn matches(&self, stderr: &str, language: &str) -> bool;
    fn recommendation(&self) -> &'static str;
}

pub struct DiagnosticEngine {
    rules: Vec<Box<dyn DiagnosticRule>>,
}

impl DiagnosticEngine {
    pub fn new() -> Self {
        Self {
            rules: vec![
                Box::new(GoMultiPackageRule),
                Box::new(GoErrorfRule),
                Box::new(JestFakeTimersRule),
                Box::new(MockResolvedValueRule),
                Box::new(TsCannotFindNameRule),
                Box::new(MissingRunTestsRule),
                Box::new(NodeMissingModuleRule),
                // New SPO rules
                Box::new(MockSideEffectRule),
                Box::new(MissingErrorHandlingRule),
                Box::new(ModuleNotInRequirementsRule),
                Box::new(TypeScriptImplicitAnyRule),
                Box::new(CerebrasContextLimitRule),
            ],
        }
    }

    /// Legacy API — returns combined hints string
    pub fn analyze(stderr: &str) -> Option<String> {
        let engine = Self::new();
        let results = engine.diagnose(stderr, "auto");
        if results.is_empty() {
            None
        } else {
            Some(results.iter().map(|(_, r)| *r).collect::<Vec<_>>().join("\n\n"))
        }
    }

    pub fn diagnose(&self, stderr: &str, language: &str) -> Vec<(&'static str, &'static str)> {
        self.rules.iter()
            .filter(|r| r.matches(stderr, language))
            .map(|r| (r.name(), r.recommendation()))
            .collect()
    }

    pub fn first_recommendation(&self, stderr: &str, language: &str) -> Option<&'static str> {
        self.rules.iter()
            .find(|r| r.matches(stderr, language))
            .map(|r| r.recommendation())
    }
}

// ── Rule #1: Go Multiple Packages ────────────────────────────
struct GoMultiPackageRule;
impl DiagnosticRule for GoMultiPackageRule {
    fn name(&self) -> &'static str { "GO_MULTI_PACKAGE" }
    fn matches(&self, stderr: &str, _lang: &str) -> bool {
        stderr.contains("found packages") && stderr.contains("in /")
    }
    fn recommendation(&self) -> &'static str {
        "CRITICAL GO ERROR: Multiple packages in same directory. Go requires all files in a directory to belong to the SAME package. Fix: Change the 'package ...' declaration so they all match, or move into separate subdirectories."
    }
}

// ── Rule #2: Go Errorf ───────────────────────────────────────
struct GoErrorfRule;
impl DiagnosticRule for GoErrorfRule {
    fn name(&self) -> &'static str { "GO_ERRORF_MISMATCH" }
    fn matches(&self, stderr: &str, _lang: &str) -> bool {
        stderr.contains("Errorf call needs") && stderr.contains("args but has")
    }
    fn recommendation(&self) -> &'static str {
        "CRITICAL GO ERROR: Argument mismatch in t.Errorf call. Ensure format verbs (%v, %d) match the number of variables passed."
    }
}

// ── Rule #3: Jest FakeTimers ─────────────────────────────────
struct JestFakeTimersRule;
impl DiagnosticRule for JestFakeTimersRule {
    fn name(&self) -> &'static str { "JEST_FAKE_TIMERS" }
    fn matches(&self, stderr: &str, _lang: &str) -> bool {
        stderr.contains("Jest did not exit one second after the test run has completed")
    }
    fn recommendation(&self) -> &'static str {
        "CRITICAL JEST ERROR: Tests hanging. Usually from improper 'jest.useFakeTimers()' with async. Fix: Use jest.runAllTimers() or avoid fake timers with async logic."
    }
}

// ── Rule #4: mockResolvedValue ───────────────────────────────
struct MockResolvedValueRule;
impl DiagnosticRule for MockResolvedValueRule {
    fn name(&self) -> &'static str { "MOCK_RESOLVED_VALUE" }
    fn matches(&self, stderr: &str, _lang: &str) -> bool {
        stderr.contains("Property 'mockResolvedValue' does not exist on type")
    }
    fn recommendation(&self) -> &'static str {
        "CRITICAL TYPESCRIPT ERROR: Typecast before using mock methods. Fix: `(func as jest.Mock).mockResolvedValue(...)`."
    }
}

// ── Rule #5: TS Cannot find name ─────────────────────────────
struct TsCannotFindNameRule;
impl DiagnosticRule for TsCannotFindNameRule {
    fn name(&self) -> &'static str { "TS_CANNOT_FIND_NAME" }
    fn matches(&self, stderr: &str, _lang: &str) -> bool {
        stderr.contains("error TS2304: Cannot find name")
    }
    fn recommendation(&self) -> &'static str {
        "CRITICAL TYPESCRIPT ERROR: Missing import. Fix: Add correct 'import { ... } from \"...\"' statement."
    }
}

// ── Rule #6: Missing run_tests ───────────────────────────────
struct MissingRunTestsRule;
impl DiagnosticRule for MissingRunTestsRule {
    fn name(&self) -> &'static str { "MISSING_RUN_TESTS" }
    fn matches(&self, stderr: &str, _lang: &str) -> bool {
        stderr.contains("Every plan MUST contain a run_tests command")
    }
    fn recommendation(&self) -> &'static str {
        "CRITICAL PROTOCOL ERROR: Plan rejected — missing 'run_tests' command. Always append run_tests at the end."
    }
}

// ── Rule #7: Node Missing Module ─────────────────────────────
struct NodeMissingModuleRule;
impl DiagnosticRule for NodeMissingModuleRule {
    fn name(&self) -> &'static str { "NODE_MISSING_MODULE" }
    fn matches(&self, stderr: &str, _lang: &str) -> bool {
        stderr.contains("Cannot find module") && stderr.contains("Require stack:")
    }
    fn recommendation(&self) -> &'static str {
        "CRITICAL NODEJS ERROR: Missing module. Fix: npm install <module> or check import paths."
    }
}

// ── Rule #8: Mock side_effect (Python) ───────────────────────
struct MockSideEffectRule;
impl DiagnosticRule for MockSideEffectRule {
    fn name(&self) -> &'static str { "MOCK_SIDE_EFFECT" }
    fn matches(&self, stderr: &str, language: &str) -> bool {
        (language == "auto" || language.to_lowercase() == "python")
            && stderr.contains("side_effect")
            && (stderr.contains("Exception") || stderr.contains("Error"))
    }
    fn recommendation(&self) -> &'static str {
        "Wrap the function body in try/except. Catch all exceptions: return {'error': str(e)} or the expected error format."
    }
}

// ── Rule #9: Missing Error Handling ──────────────────────────
struct MissingErrorHandlingRule;
impl DiagnosticRule for MissingErrorHandlingRule {
    fn name(&self) -> &'static str { "MISSING_ERROR_HANDLING" }
    fn matches(&self, stderr: &str, _lang: &str) -> bool {
        (stderr.contains("Exception") || stderr.contains("Error"))
            && (stderr.contains("not caught") || stderr.contains("Traceback"))
            && !stderr.contains("AssertionError")
    }
    fn recommendation(&self) -> &'static str {
        "Add try/except around the risky operation. Return the expected error format instead of raising."
    }
}

// ── Rule #10: ModuleNotFoundError ────────────────────────────
struct ModuleNotInRequirementsRule;
impl DiagnosticRule for ModuleNotInRequirementsRule {
    fn name(&self) -> &'static str { "MODULE_NOT_IN_REQUIREMENTS" }
    fn matches(&self, stderr: &str, _lang: &str) -> bool {
        stderr.contains("ModuleNotFoundError") || stderr.contains("No module named")
    }
    fn recommendation(&self) -> &'static str {
        "Add missing module to requirements.txt and run: pip install -r requirements.txt before testing."
    }
}

// ── Rule #11: TypeScript implicit any ────────────────────────
struct TypeScriptImplicitAnyRule;
impl DiagnosticRule for TypeScriptImplicitAnyRule {
    fn name(&self) -> &'static str { "TS_IMPLICIT_ANY" }
    fn matches(&self, stderr: &str, language: &str) -> bool {
        let lang = if language == "auto" { "" } else { language };
        let is_ts = lang.is_empty() || lang.to_lowercase() == "typescript" || lang.to_lowercase() == "ts";
        is_ts && stderr.contains("implicitly has an 'any' type")
    }
    fn recommendation(&self) -> &'static str {
        "Add explicit type annotation. Example: change (param) to (param: string)."
    }
}

// ── Rule #12: Cerebras Context Limit ─────────────────────────
struct CerebrasContextLimitRule;
impl DiagnosticRule for CerebrasContextLimitRule {
    fn name(&self) -> &'static str { "CEREBRAS_CONTEXT_LIMIT" }
    fn matches(&self, stderr: &str, _lang: &str) -> bool {
        stderr.contains("maximum context") || stderr.contains("context length")
            || stderr.contains("8192") || stderr.contains("65536") || stderr.contains("token limit")
    }
    fn recommendation(&self) -> &'static str {
        "Prompt exceeds Cerebras context limit (8K for Llama, 64K for Qwen3). Split task into smaller chunks or switch to OpenRouter."
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn test_mock_side_effect_python() {
        let rule = MockSideEffectRule;
        assert!(rule.matches("side_effect Exception: invalid", "python"));
        assert!(!rule.matches("side_effect Exception: invalid", "rust"));
    }

    #[test]
    fn test_module_not_found() {
        let rule = ModuleNotInRequirementsRule;
        assert!(rule.matches("ModuleNotFoundError: No module named 'requests'", "python"));
    }

    #[test]
    fn test_ts_implicit_any() {
        let rule = TypeScriptImplicitAnyRule;
        assert!(rule.matches("implicitly has an 'any' type", "typescript"));
        assert!(!rule.matches("implicitly has an 'any' type", "python"));
    }

    #[test]
    fn test_cerebras_context() {
        let rule = CerebrasContextLimitRule;
        assert!(rule.matches("maximum context length exceeded", "rust"));
    }

    #[test]
    fn test_engine_multiple_matches() {
        let engine = DiagnosticEngine::new();
        let results = engine.diagnose(
            "No module named 'requests'\nside_effect ValueError",
            "python"
        );
        let names: Vec<&str> = results.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"MODULE_NOT_IN_REQUIREMENTS"));
        assert!(names.contains(&"MOCK_SIDE_EFFECT"));
    }

    #[test]
    fn test_legacy_analyze_api() {
        let result = DiagnosticEngine::analyze("found packages main and foo in /tmp");
        assert!(result.is_some());
    }

    #[test]
    fn test_assertion_error_excluded() {
        let rule = MissingErrorHandlingRule;
        assert!(!rule.matches("AssertionError: expected 5 got 4", "python"));
        assert!(rule.matches("ValueError: invalid\nTraceback", "python"));
    }
}
