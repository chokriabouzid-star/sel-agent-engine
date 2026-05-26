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
    InfraError, // v6.4: connection error / rate limit / pip timeout
    PatchError, // v6.6: search block not found / validation failed
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
        // Infra errors
        if s.contains("Connection error")
            || s.contains("rate limit")
            || s.contains("Rate limit")
            || s.contains("429")
            || s.contains("503")
            || s.contains("502")
            || s.contains("Timeout after")
            || s.contains("error sending request")
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

    pub fn repair_hint(&self) -> &str {
        match self {
            Self::SyntaxError => "SYNTAX ERROR: Fix syntax only.",
            Self::ImportError => "IMPORT ERROR: Module not found.",
            Self::AssertionError => "ASSERTION ERROR: Logic is wrong.",
            Self::TypeError => "TYPE ERROR: Wrong types used.",
            Self::CollectionError => "COLLECTION ERROR: No tests found.",
            Self::BuildError => "BUILD ERROR: Compilation failed.",
            Self::NodeTestError => "NODE TEST ERROR: Use Node.js assert, not Jest.",
            Self::DatabaseError => "DATABASE ERROR: SQLite table missing.",
            Self::FlaskConcurrency => "FLASK CONTEXT ERROR: Missing app context.",
            Self::PatchError => "PATCH ERROR: Search block mismatch.",
            Self::InfraError => "INFRA ERROR: API/Network issue.",
            Self::Unknown => "Fix the errors shown.",
        }
    }

    pub fn max_attempts(&self) -> u8 {
        match self {
            Self::PatchError => 2,
            Self::InfraError => 0,
            _ => 3,
        }
    }
}
