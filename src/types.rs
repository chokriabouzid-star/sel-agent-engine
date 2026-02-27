// src/types.rs — v0.4: الأنواع الأساسية

use std::path::PathBuf;
use std::collections::HashSet;

// ══════════════════════════════════════════════════════
// State Machine
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum AgentState {
    Planning,
    Executing,
    Repairing,
    Done,
    Failed(String),
}

// ══════════════════════════════════════════════════════
// سياق التنفيذ — الحالة الفعلية للنظام
// ══════════════════════════════════════════════════════

#[derive(Debug, Default)]
pub struct ExecutionContext {
    pub tests_passed:      bool,
    pub last_exit_code:    Option<i32>,
    pub failed_steps:      Vec<FailedStep>,
    pub repair_attempts:   u8,
    pub successful_hashes: HashSet<String>,
    pub max_repairs:       u8,
}

impl ExecutionContext {
    pub fn new(max_repairs: u8) -> Self {
        Self { max_repairs, successful_hashes: std::collections::HashSet::new(), ..Default::default() }
    }
    pub fn reset_for_repair(&mut self) {
        self.tests_passed   = false;
        self.last_exit_code = None;
        self.failed_steps.clear();
    }
    pub fn has_failures(&self) -> bool {
        !self.failed_steps.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct FailedStep {
    pub step_index: usize,
    pub label:      String,
    pub stderr:     String,
    pub exit_code:  i32,
}

// ══════════════════════════════════════════════════════
// نتيجة التنفيذ
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ExecResult {
    pub success:     bool,
    pub exit_code:   i32,
    pub stdout:      String,
    pub stderr:      String,
    pub duration_ms: u64,
}

impl ExecResult {
    pub fn ok(msg: impl Into<String>) -> Self {
        Self { success: true,  exit_code: 0, stdout: msg.into(), stderr: String::new(), duration_ms: 0 }
    }
    pub fn fail(msg: impl Into<String>) -> Self {
        Self { success: false, exit_code: 1, stdout: String::new(), stderr: msg.into(), duration_ms: 0 }
    }
}

// ══════════════════════════════════════════════════════
// Message
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub role:    String,
    pub content: String,
}

impl Message {
    pub fn system(s: impl Into<String>) -> Self { Self { role: "system".into(),    content: s.into() } }
    pub fn user(s:   impl Into<String>) -> Self { Self { role: "user".into(),      content: s.into() } }
}

// ══════════════════════════════════════════════════════
// خطأ الأمان
// ══════════════════════════════════════════════════════

#[derive(Debug)]
pub enum SafetyError {
    PathTraversal(String),
    BlockedCommand(String),
    WorkspaceEscape(String),
}

impl std::fmt::Display for SafetyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::PathTraversal(s)   => write!(f, "Path traversal: {}", s),
            Self::BlockedCommand(s)  => write!(f, "Blocked command: {}", s),
            Self::WorkspaceEscape(s) => write!(f, "Workspace escape: {}", s),
        }
    }
}

// ══════════════════════════════════════════════════════
// Failure Classification
// ══════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum FailureKind {
    SyntaxError,
    ImportError,
    AssertionError,
    TypeError,
    CollectionError,
    BuildError,
    Unknown,
}

impl FailureKind {
    pub fn classify(stderr: &str) -> Self {
        let s = stderr;
        // Go errors
        if s.contains("undefined:") || s.contains("cannot use") {
            return Self::TypeError;
        }
        if s.contains("syntax error:") && (s.contains(".go:") || s.contains("unexpected")) {
            return Self::SyntaxError;
        }
        if s.contains("FAIL	") || s.contains("--- FAIL") {
            return Self::AssertionError;
        }
        // Rust project structure errors
        if s.contains("could not find `Cargo.toml`") || s.contains("could not find Cargo.toml") {
            return Self::BuildError;
        }
        // Rust assertion failures
        if s.contains("left") && s.contains("right") && s.contains("panicked") {
            return Self::AssertionError;
        }
        // Rust errors
        if s.contains("error[E") || (s.contains("error:") && s.contains("-->")) {
            // Rust type/borrow errors
            if s.contains("E0308") || s.contains("mismatched types") || s.contains("E0507") {
                return Self::TypeError;
            }
            return Self::BuildError;
        }
        if s.contains("thread") && s.contains("panicked") {
            return Self::AssertionError;
        }
        if s.contains("FAILED") && s.contains("test result:") {
            return Self::AssertionError;
        }
        // Python errors
        if s.contains("ModuleNotFoundError") || s.contains("ImportError while importing")
            || s.contains("No module named") {
            return Self::ImportError;
        }
        if s.contains("SyntaxError") || s.contains("was never closed") {
            return Self::SyntaxError;
        }
        if s.contains("collected 0 items") {
            return Self::CollectionError;
        }
        if s.contains("TypeError") {
            return Self::TypeError;
        }
        if s.contains("AssertionError") {
            return Self::AssertionError;
        }
        if s.contains("error[E") || s.contains("error: ") && s.contains("-->") {
            return Self::BuildError;
        }
        Self::Unknown
    }

    pub fn repair_hint(&self) -> &str {
        match self {
            Self::SyntaxError =>
                "SYNTAX ERROR: Fix syntax only. Do NOT change logic or reinstall packages.",
            Self::ImportError =>
                "IMPORT ERROR: Module not found. Either install it with pip or use stdlib alternative.",
            Self::AssertionError =>
                "ASSERTION ERROR: Logic is wrong. Fix the implementation, not the test.",
            Self::TypeError =>
                "TYPE ERROR: Wrong types used. Check function signatures and return types.",
            Self::CollectionError =>
                "COLLECTION ERROR: pytest found 0 tests. Ensure test functions start with test_",
            Self::BuildError =>
                "BUILD ERROR: Compilation failed. Fix the compile errors shown.",
            Self::Unknown =>
                "Fix the errors shown above.",
        }
    }
}

