// src/failure.rs  v1.0: Failure Classification
//

#[derive(Debug, Clone, PartialEq)]
pub enum FailureKind {
    SyntaxError,
    ImportError,
    AssertionError,
    TypeError,
    CollectionError,
    BuildError,
    DatabaseError,
    NodeTestError,
    FlaskConcurrency,
    InfraError,   // v6.4: connection error / rate limit / pip timeout
    PatchError,   // v6.6: search block not found / validation failed
    MissingTests, // v9.3.3: cargo/pytest ran 0 tests when tests required
    Unknown,
}

impl FailureKind {
    pub fn classify(stderr: &str) -> Self {
        let s = stderr;
        // Patch errors
        if s.contains("search block not found")
            || s.contains("patch_file validation failed")
            || s.contains("Patch changed too many lines")
            || s.contains("search block is empty")
        {
            return Self::PatchError;
        }
        // Missing tests (Rust: running 0 tests, passed == 0)
        if (s.contains("running 0 tests")
            || s.contains("0 passed, 0 failed")
            || s.contains("0 passed; 0 failed; 0 ignored"))
            && !s.contains("error[E")
            && !s.contains("error:")
        {
            return Self::MissingTests;
        }
        // Infra errors: Must be strictly contextual to avoid catching mocked HTTP status codes in user tests.
        // Bare numbers like "503" or "429" will trigger false positives in API/web testing tasks.
        if s.contains("Connection error")
            || s.contains("error sending request")
            || s.contains("Timeout after ") // Agent's own execution timeout wrapper
            || s.contains("ERR! network")   // npm network error
            || s.contains("ECONNREFUSED")
            || s.contains("ReadTimeoutError")
            || s.contains("HTTP Error 429") || s.contains("HTTP 429") || s.contains("429 Too Many Requests")
            || s.contains("HTTP Error 502") || s.contains("HTTP 502") || s.contains("502 Bad Gateway")
            || s.contains("HTTP Error 503") || s.contains("HTTP 503") || s.contains("503 Service Unavailable")
            || s.contains("Rate limit exceeded") || s.contains("rate_limit_exceeded") || s.contains("API rate limit")
        {
            return Self::InfraError;
        }
        // Go errors
        if s.contains("undefined:")
            || s.contains("cannot use")
            || s.contains("no required module")
            || s.contains("cannot find package")
        {
            return Self::TypeError;
        }
        // P1 (2026-09-15): literal `\"` in Go source — go/scanner says "illegal character",
        // cmd/compile says "invalid character"; both carry U+005C.
        if (s.contains("illegal character U+") || s.contains("invalid character U+"))
            && s.contains(".go")
        {
            return Self::SyntaxError;
        }
        if s.contains("syntax error:") && (s.contains(".go:") || s.contains("unexpected")) {
            return Self::SyntaxError;
        }
        if s.contains("FAIL	") || s.contains("--- FAIL") {
            return Self::AssertionError;
        }
        // Rust errors
        if s.contains("could not find `Cargo.toml`")
            || s.contains("error[E")
            || s.contains("error:") && s.contains("-->")
        {
            if s.contains("E0308") || s.contains("mismatched types") {
                return Self::TypeError;
            }
            return Self::BuildError;
        }
        // Python errors
        if s.contains("ModuleNotFoundError")
            || s.contains("ImportError")
            || s.contains("No module named")
        {
            return Self::ImportError;
        }
        if s.contains("SyntaxError") || s.contains("was never closed") {
            return Self::SyntaxError;
        }
        // v7.5.1: NameError (standalone)  missing import or undefined name
        if s.contains("NameError") && s.contains("is not defined") {
            return Self::ImportError;
        }
        if s.contains("collected 0 items") || s.contains("Interrupted: 1 error during collection") {
            if s.contains("NameError") || s.contains("is not defined") {
                return Self::ImportError;
            }
            return Self::CollectionError;
        }
        if s.contains("AttributeError") || s.contains("TypeError") {
            return Self::TypeError;
        }
        if s.contains("AssertionError") {
            return Self::AssertionError;
        }
        if s.contains("Flask") || s.contains("app_ctx") || s.contains("application context") {
            return Self::FlaskConcurrency;
        }
        if s.contains("no such table") || s.contains("sqlite3") {
            return Self::DatabaseError;
        }
        if s.contains("ReferenceError") {
            return Self::NodeTestError;
        }
        Self::Unknown
    }

    #[allow(dead_code)] // v7.3: diagnostic repair hints
    pub fn repair_hint(&self) -> &str {
        match self {
            Self::SyntaxError => "SYNTAX ERROR: Fix syntax only.",
            Self::ImportError => "IMPORT ERROR: Module not found.",
            Self::AssertionError => "ASSERTION ERROR: Logic is wrong.",
            Self::TypeError => "TYPE ERROR: Wrong types used.",
            Self::CollectionError => "COLLECTION ERROR: No tests found.",
            Self::MissingTests => "MISSING TESTS: 0 tests ran. Add #[cfg(test)] mod tests { } with at least one #[test] fn.",
            Self::BuildError => "BUILD ERROR: Compilation failed.",
            Self::NodeTestError => "NODE TEST ERROR: Use Node.js assert, not Jest.",
            Self::DatabaseError => "DATABASE ERROR: SQLite table missing.",
            Self::FlaskConcurrency => "FLASK CONTEXT ERROR: Missing app context.",
            Self::PatchError => "PATCH ERROR: Search block mismatch.",
            Self::InfraError => "INFRA ERROR: API/Network issue.",
            Self::Unknown => "Fix the errors shown.",
        }
    }
    #[allow(dead_code)] // v7.3: custom failure attempts policy
    pub fn max_attempts(&self) -> u8 {
        match self {
            Self::PatchError => 2,
            Self::InfraError => 0,
            Self::MissingTests => 2,
            _ => 3,
        }
    }
}

#[cfg(test)]
mod p1_failure_tests {
    use super::FailureKind;

    #[test]
    fn p1_go_u005c_is_syntax_error_not_unknown() {
        let err =
            "COMPILE ERROR in 'main_test.go':\n./main_test.go:3:8: illegal character U+005C '\\'";
        assert_eq!(FailureKind::classify(err), FailureKind::SyntaxError);
        let err2 = "./main_test.go:3:8: invalid character U+005C '\\'";
        assert_eq!(FailureKind::classify(err2), FailureKind::SyntaxError);
    }
}
