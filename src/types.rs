// src/types.rs — v1.3: الأنواع الأساسية

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
    pub start_time:        Option<std::time::Instant>,
}

impl ExecutionContext {
    pub fn new(max_repairs: u8) -> Self {
        Self { max_repairs, start_time: None, successful_hashes: std::collections::HashSet::new(), ..Default::default() }
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
    pub step_index:   usize,
    pub label:        String,
    pub stderr:       String,
    pub exit_code:    i32,
    pub culprit_file: Option<String>,  // الملف المسؤول عن الخطأ
}

impl FailedStep {
    pub fn extract_culprit(stderr: &str) -> Option<String> {
        for line in stderr.lines() {
            // Python traceback: File "/path/file.py", line 42
            if line.contains("File \"") && line.contains(".py") {
                if let Some(s) = line.find("File \"") {
                    let rest = &line[s+6..];
                    if let Some(e) = rest.find('"') {
                        let path = &rest[..e];
                        if !path.contains("venv") && !path.contains("site-packages") {
                            let base = path.rfind('/').map(|i| i+1).unwrap_or(0);
                            let name = &path[base..];
                            if !name.starts_with("test_") {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
            // pytest: service.py:1: in <module>
            if line.contains(".py:") && line.contains(": in ") {
                // تجاهل مسارات stdlib وvenv
                if line.contains("/usr/lib") || line.contains("venv/") || line.contains("site-packages") {
                    continue;
                }
                if let Some(pos) = line.find(".py:") {
                    let start = line[..pos].rfind(|c: char| c == '/' || c == ' ' || c == '\t').map(|i| i+1).unwrap_or(0);
                    let name = &line[start..pos+3];
                    if !name.starts_with("test_") && !name.starts_with("__") {
                        return Some(name.to_string());
                    }
                }
            }
            // Rust: --> src/lib.rs:42:5
            if line.trim_start().starts_with("--> ") {
                let rest = line.trim_start().trim_start_matches("--> ");
                if let Some(colon) = rest.find(':') {
                    let path = &rest[..colon];
                    let base = path.rfind('/').map(|i| i+1).unwrap_or(0);
                    return Some(path[base..].to_string());
                }
            }
            // Go: file.go:42
            if line.contains(".go:") {
                if let Some(pos) = line.find(".go:") {
                    let start = line[..pos].rfind(|c: char| c == '/' || c == ' ').map(|i| i+1).unwrap_or(0);
                    return Some(line[start..pos+3].to_string());
                }
            }
            // Node.js: file.js:42
            if line.contains(".js:") && !line.contains("node_modules") {
                if let Some(pos) = line.find(".js:") {
                    let start = line[..pos].rfind(|c: char| c == '/' || c == ' ').map(|i| i+1).unwrap_or(0);
                    return Some(line[start..pos+3].to_string());
                }
            }
        }
        None
    }
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
    DatabaseError,
    NodeTestError,
    FlaskConcurrency,
    Unknown,
}

impl FailureKind {
    pub fn classify(stderr: &str) -> Self {
        let s = stderr;
        // Go errors
        if s.contains("undefined:") || s.contains("cannot use") || s.contains("no required module") || s.contains("cannot find package") {
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
        if s.contains("LookupError") && (s.contains("flask") || s.contains("app_ctx") || s.contains("application context"))
            || s.contains("RuntimeError") && s.contains("Working outside of application context")
            || s.contains("RuntimeError") && s.contains("Working outside of request context")
            || s.contains("Push an application context") {
            return Self::FlaskConcurrency;
        }
        if s.contains("ReferenceError: test is not defined")
            || s.contains("ReferenceError: describe is not defined")
            || s.contains("ReferenceError: expect is not defined") {
            return Self::NodeTestError;
        }
        if s.contains("OperationalError") || s.contains("no such table")
            || s.contains("readonly database") || s.contains("sqlite3")
            || s.contains("sqlalchemy") {
            return Self::DatabaseError;
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
            Self::NodeTestError =>
                "NODE TEST ERROR: Do NOT use Jest/Mocha syntax (test/describe/expect).                  Use only Node.js built-in assert module.                  Example: const assert = require('assert'); assert.strictEqual(add(2,3), 5);",
            Self::DatabaseError =>
                "DATABASE ERROR: The test database is not set up correctly.                  You MUST use this exact pattern in test_main.py:
                 
                 from sqlalchemy import create_engine
                 from sqlalchemy.orm import sessionmaker
                 from main import app, Base, get_db
                 from fastapi.testclient import TestClient
                 
                 SQLALCHEMY_TEST_URL = 'sqlite:///:memory:'
                 engine = create_engine(SQLALCHEMY_TEST_URL, connect_args={'check_same_thread': False})
                 TestingSessionLocal = sessionmaker(bind=engine)
                 
                 def override_get_db():
                     db = TestingSessionLocal()
                     try: yield db
                     finally: db.close()
                 
                 app.dependency_overrides[get_db] = override_get_db
                 Base.metadata.create_all(bind=engine)
                 client = TestClient(app)
                 
                 IMPORTANT: main.py must have get_db() as a dependency injection function.",
            Self::FlaskConcurrency =>
                "FLASK CONTEXT ERROR: Code is running outside Flask application context.                 
YOU MUST fix the test file using one of these patterns:                 

PATTERN A — pytest fixture (recommended):                 
  import pytest                 
  from main import app                 
  @pytest.fixture                 
  def client():                 
      app.config['TESTING'] = True                 
      with app.test_client() as c:                 
          yield c                 
  def test_route(client):                 
      r = client.get('/')                 
      assert r.status_code == 200                 

PATTERN B — app_context manually:                 
  with app.app_context():                 
      # code that needs app context                 

NEVER call db or app internals outside app context.",
            Self::Unknown =>
                "Fix the errors shown above.",
        }
    }
}

impl ExecutionContext {
    pub fn load_hashes(&mut self, workspace: &std::path::Path) {
        let path = workspace.join(".sel_hashes");
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let h = line.trim();
                if !h.is_empty() { self.successful_hashes.insert(h.to_string()); }
            }
        }
    }

    pub fn save_hashes(&self, workspace: &std::path::Path) {
        let path = workspace.join(".sel_hashes");
        let content = self.successful_hashes.iter().cloned().collect::<Vec<_>>().join("\n");
        let _ = std::fs::write(path, content);
    }
}
