#![allow(clippy::empty_docs)]
//! SEL Agent v6.3 - Constraint Engine
//!
//! :
//! 1. Environment Lock:  pytest  Node  npm  Python
//! 2. Config Deduplication:   jest.config.js   package.json  jest
//! 3. Dependency Normalization:  versions   pinned stacks
//! 4. Fatal Constraints:
//!
//!  :
//! 1 environment::enforce  2 config::deduplicate  3 dependencies::normalize

use crate::context::Scanner;
use crate::protocol::Cmd;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

// ============================================================================
//
// ============================================================================

///   -     Scanner
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectEnv {
    Node { has_typescript: bool },
    Python,
    Rust,
    Go,
    Unknown,
}

impl ProjectEnv {
    ///    workspace
    pub fn detect(workspace: &Path) -> Self {
        if workspace.join("Cargo.toml").exists() {
            return ProjectEnv::Rust;
        }
        if workspace.join("go.mod").exists() {
            return ProjectEnv::Go;
        }
        if workspace.join("requirements.txt").exists() || workspace.join("pyproject.toml").exists()
        {
            return ProjectEnv::Python;
        }
        if workspace.join("package.json").exists() {
            let has_ts = Scanner::has_ts_files(workspace);
            return ProjectEnv::Node {
                has_typescript: has_ts,
            };
        }
        ProjectEnv::Unknown
    }
}

///    disk -
#[derive(Debug, Clone, Default)]
pub struct ProjectState {
    #[allow(dead_code)]
    pub files: HashSet<String>,
    pub has_jest_config: bool,
    #[allow(dead_code)]
    pub has_tsconfig: bool,
    pub has_package_json: bool,
    pub jest_in_pkg_json: bool,
}

impl ProjectState {
    ///  workspace
    pub fn scan(workspace: &Path) -> Self {
        let mut files = HashSet::new();
        let has_jest_config;

        let has_package_json;
        let jest_in_pkg_json;

        //  jest.config.js
        if workspace.join("jest.config.js").exists() {
            files.insert("jest.config.js".to_string());
            has_jest_config = true;
        } else if workspace.join("jest.config.ts").exists() {
            files.insert("jest.config.ts".to_string());
            has_jest_config = true;
        } else {
            has_jest_config = false;
        } //  tsconfig.json
        let has_tsconfig = if workspace.join("tsconfig.json").exists() {
            files.insert("tsconfig.json".to_string());
            true
        } else {
            false
        };

        //  package.json  jest field
        let pkg_path = workspace.join("package.json");
        if pkg_path.exists() {
            files.insert("package.json".to_string());
            has_package_json = true;
            jest_in_pkg_json = Self::check_jest_in_package_json(&pkg_path);
        } else {
            has_package_json = false;
            jest_in_pkg_json = false;
        }

        Self {
            files,
            has_jest_config,
            has_tsconfig,
            has_package_json,
            jest_in_pkg_json,
        }
    }

    fn check_jest_in_package_json(path: &Path) -> bool {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return false,
        };
        if let Ok(json) = serde_json::from_str::<Value>(&content) {
            if json.get("jest").is_some() {
                return true;
            }
        }
        false
    }
}

///
#[derive(Debug, Clone)]
pub enum ConstraintResult {
    ///  -   ( )
    Ok(Vec<Cmd>),
    ///   -
    Fatal(String),
}

// ============================================================================
// Pinned Stacks -
// ============================================================================

/// Pinned Stack  TypeScript + Jest
const TS_JEST_STACK: &str = "typescript@5.3.3 ts-jest@29.1.1 jest@29.7.0 @types/jest@29.5.11";

///  package.json   TypeScript
const CORRECT_PACKAGE_JSON_TS: &str = r#"{
  "scripts": {
    "test": "jest",
    "build": "tsc"
  },
  "devDependencies": {
    "typescript": "5.3.3",
    "ts-jest": "29.1.1",
    "jest": "29.7.0",
    "@types/jest": "29.5.11"
  },
  "jest": {
    "preset": "ts-jest",
    "testEnvironment": "node"
  }
}"#;

// ============================================================================
//  1: Environment Lock
// ============================================================================

///
fn enforce_environment(plan: Vec<Cmd>, env: &ProjectEnv) -> Vec<Cmd> {
    let mut filtered = Vec::new();

    for cmd in plan {
        match cmd {
            Cmd::Run { ref command } => {
                let cmd_lower = command.to_lowercase();
                let is_allowed = match env {
                    ProjectEnv::Node { .. } => {
                        //   Python  Node
                        !(cmd_lower.contains("pytest")
                            || cmd_lower.contains("venv")
                            || cmd_lower.contains("pip")
                            || cmd_lower.contains("python"))
                    }
                    ProjectEnv::Python => {
                        //   Node  Python
                        !(cmd_lower.contains("npm")
                            || cmd_lower.contains("npx")
                            || cmd_lower.contains("node"))
                    }
                    ProjectEnv::Rust => {
                        //  npm  pip  Rust
                        !(cmd_lower.contains("npm")
                            || cmd_lower.contains("pip")
                            || cmd_lower.contains("pytest"))
                    }
                    ProjectEnv::Go => {
                        //  npm  pip  Go
                        !(cmd_lower.contains("npm") || cmd_lower.contains("pip"))
                    }
                    ProjectEnv::Unknown => true,
                };

                if is_allowed {
                    filtered.push(cmd);
                } else {
                    eprintln!(" Constraint: blocked '{}' (wrong environment)", command);
                }
            }
            _ => filtered.push(cmd),
        }
    }

    filtered
}

// ============================================================================
//  2: Config Deduplication -  jest.config.js
// ============================================================================

///  WriteFile  package.json  jest.config.js
fn intercept_write_file(
    path: &str,
    content: &str,
    state: &ProjectState,
    env: &ProjectEnv,
) -> Option<Cmd> {
    let path_lower = path.to_lowercase();

    //  1:  jest.config.js
    if path_lower.ends_with("jest.config.js") || path_lower.ends_with("jest.config.ts") {
        // :  package.json   jest field
        if state.has_package_json && state.jest_in_pkg_json {
            eprintln!(
                " Constraint: blocked write '{}' (jest already in package.json)",
                path
            );
            return None; //
        }

        //     package.json    jest field    package.json
        if state.has_package_json {
            eprintln!(" Constraint: converting jest.config.js  merge into package.json");
            //   jest config  package.json
            //    normalize_package_json
            return None; //   normalize_package_json
        }
    }

    //  2:  package.json -
    if path_lower.ends_with("package.json") {
        return Some(Cmd::WriteFile {
            path: path.to_string(),
            content: normalize_package_json(content, state, env).to_string(),
        });
    }

    // For all other files, return the command unmodified
    Some(Cmd::WriteFile {
        path: path.to_string(),
        content: content.to_string(),
    })
}

///  package.json:  pinned stack  jest field    jest.config.js
fn normalize_package_json(content: &str, state: &ProjectState, env: &ProjectEnv) -> String {
    let mut json: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => {
            eprintln!(" Constraint: invalid package.json, using template");
            return CORRECT_PACKAGE_JSON_TS.to_string();
        }
    };

    //   TypeScript projects
    let is_ts_project = match env {
        ProjectEnv::Node { has_typescript } => *has_typescript,
        _ => false,
    };

    if !is_ts_project {
        return content.to_string();
    }

    // 1.  pinned stack  devDependencies
    let dev_deps = json
        .get_mut("devDependencies")
        .and_then(|d| d.as_object_mut());

    if let Some(deps) = dev_deps {
        //  typescript
        deps.insert("typescript".to_string(), Value::String("5.3.3".to_string()));
        deps.insert("ts-jest".to_string(), Value::String("29.1.1".to_string()));
        deps.insert("jest".to_string(), Value::String("29.7.0".to_string()));
        deps.insert(
            "@types/jest".to_string(),
            Value::String("29.5.11".to_string()),
        );
    } else {
        //    devDependencies
        let mut deps = serde_json::Map::new();
        deps.insert("typescript".to_string(), Value::String("5.3.3".to_string()));
        deps.insert("ts-jest".to_string(), Value::String("29.1.1".to_string()));
        deps.insert("jest".to_string(), Value::String("29.7.0".to_string()));
        deps.insert(
            "@types/jest".to_string(),
            Value::String("29.5.11".to_string()),
        );
        json["devDependencies"] = Value::Object(deps);
    }

    // 2.  scripts
    let scripts = json.get_mut("scripts").and_then(|s| s.as_object_mut());
    if let Some(scripts) = scripts {
        //      script
        if !scripts.contains_key("test") {
            scripts.insert("test".to_string(), Value::String("jest".to_string()));
        }
        if !scripts.contains_key("build") {
            scripts.insert("build".to_string(), Value::String("tsc".to_string()));
        }
    } else {
        let mut scripts = serde_json::Map::new();
        scripts.insert("test".to_string(), Value::String("jest".to_string()));
        scripts.insert("build".to_string(), Value::String("tsc".to_string()));
        json["scripts"] = Value::Object(scripts);
    }

    // 3.  jest config  package.json (    jest.config.js)
    if !state.has_jest_config {
        let jest_config = json.get_mut("jest").and_then(|j| j.as_object_mut());
        if jest_config.is_none() {
            let mut jest = serde_json::Map::new();
            jest.insert("preset".to_string(), Value::String("ts-jest".to_string()));
            jest.insert(
                "testEnvironment".to_string(),
                Value::String("node".to_string()),
            );
            json["jest"] = Value::Object(jest);
        }
    } else {
        //    jest.config.js  jest field  package.json
        if json.get("jest").is_some() {
            eprintln!(
                " Constraint: removing 'jest' field from package.json (jest.config.js exists)"
            );
            json.as_object_mut().and_then(|obj| obj.remove("jest"));
        }
    }

    // 4.     'type': 'module' (   jest)
    if let Some(r#type) = json.get("type") {
        if r#type == "module" {
            eprintln!(" Constraint: removing 'type': 'module' from package.json");
            json.as_object_mut().and_then(|obj| obj.remove("type"));
        }
    }

    serde_json::to_string_pretty(&json).unwrap_or_else(|_| content.to_string())
}

// ============================================================================
//  3: Dependency Normalization
// ============================================================================

///   npm install  pinned stacks
fn normalize_dependencies(plan: Vec<Cmd>, env: &ProjectEnv) -> Vec<Cmd> {
    let is_ts_project = match env {
        ProjectEnv::Node { has_typescript } => *has_typescript,
        _ => false,
    };

    if !is_ts_project {
        return plan;
    }

    let mut normalized = Vec::new();

    for cmd in plan {
        match cmd {
            Cmd::Run { command } => {
                let cmd_lower = command.to_lowercase();

                // npm install typescript ts-jest ...
                if cmd_lower.contains("npm install")
                    && (cmd_lower.contains("typescript") || cmd_lower.contains("ts-jest"))
                {
                    eprintln!(" Constraint: normalizing npm install  using pinned stack");
                    normalized.push(Cmd::Run {
                        command: format!("npm install {}", TS_JEST_STACK),
                    });
                } else {
                    normalized.push(Cmd::Run { command });
                }
            }
            _ => normalized.push(cmd),
        }
    }

    normalized
}

// ============================================================================
//  4: Fatal Constraints
// ============================================================================

///  fatal errors -
fn check_fatal(plan: &[Cmd], env: &ProjectEnv) -> Option<String> {
    let mut has_npm = false;
    let mut has_pip = false;

    for cmd in plan {
        if let Cmd::Run { command } = cmd {
            let cmd_lower = command.to_lowercase();
            if cmd_lower.contains("npm") || cmd_lower.contains("npx") {
                has_npm = true;
            }
            if cmd_lower.contains("pip") || cmd_lower.contains("pytest") {
                has_pip = true;
            }
        }
    }

    // :  Python    npm
    if let ProjectEnv::Python = env {
        if has_npm && !has_pip {
            return Some(format!(
                "Fatal: Python project but plan contains npm commands (has_npm={}, has_pip={})",
                has_npm, has_pip
            ));
        }
    }

    // :  Node    pip
    if let ProjectEnv::Node { .. } = env {
        if has_pip && !has_npm {
            return Some(format!(
                "Fatal: Node project but plan contains pip commands (has_pip={}, has_npm={})",
                has_pip, has_npm
            ));
        }
    }

    None
}

// ============================================================================
//
// ============================================================================

///
pub fn apply(plan: Vec<Cmd>, env: &ProjectEnv, state: &ProjectState) -> ConstraintResult {
    // 1.  fatal
    if let Some(reason) = check_fatal(&plan, env) {
        return ConstraintResult::Fatal(reason);
    }

    // 2. Environment Lock
    let plan = enforce_environment(plan, env);

    // 3. Config Deduplication -  WriteFile
    let mut processed = Vec::new();
    for cmd in plan {
        match cmd {
            Cmd::WriteFile {
                ref path,
                ref content,
            } => {
                if let Some(new_cmd) = intercept_write_file(path, content, state, env) {
                    processed.push(new_cmd);
                }
                //   intercept_write_file  None   ( )
            }
            _ => processed.push(cmd),
        }
    }

    // 4. Dependency Normalization
    let plan = normalize_dependencies(processed, env);

    ConstraintResult::Ok(plan)
}

// ============================================================================
//
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_detect_rust() {
        let dir = tempdir().expect("test setup/use should succeed");
        std::fs::write(dir.path().join("Cargo.toml"), "[package]")
            .expect("test setup/use should succeed");
        assert_eq!(ProjectEnv::detect(dir.path()), ProjectEnv::Rust);
    }

    #[test]
    fn test_detect_python() {
        let dir = tempdir().expect("test setup/use should succeed");
        std::fs::write(dir.path().join("requirements.txt"), "")
            .expect("test setup/use should succeed");
        assert_eq!(ProjectEnv::detect(dir.path()), ProjectEnv::Python);
    }

    #[test]
    fn test_detect_node() {
        let dir = tempdir().expect("test setup/use should succeed");
        std::fs::write(dir.path().join("package.json"), "{}")
            .expect("test setup/use should succeed");
        assert_eq!(
            ProjectEnv::detect(dir.path()),
            ProjectEnv::Node {
                has_typescript: false
            }
        );
    }

    #[test]
    fn test_enforce_environment_python() {
        let env = ProjectEnv::Python;
        let plan = vec![
            Cmd::Run {
                command: "npm install".to_string(),
            },
            Cmd::Run {
                command: "pytest".to_string(),
            },
        ];

        let filtered = enforce_environment(plan, &env);
        assert_eq!(filtered.len(), 1);
        match &filtered[0] {
            Cmd::Run { command } => assert_eq!(command, "pytest"),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_normalize_package_json() {
        let _dir = tempdir().expect("test setup/use should succeed");
        let state = ProjectState {
            has_jest_config: false,
            ..Default::default()
        };
        let env = ProjectEnv::Node {
            has_typescript: true,
        };

        let input = r#"{"name": "test"}"#;
        let output = normalize_package_json(input, &state, &env);

        assert!(output.contains("typescript\": \"5.3.3\""));
        assert!(output.contains("ts-jest\": \"29.1.1\""));
        assert!(output.contains("\"jest\""));
    }

    #[test]
    fn test_remove_jest_when_config_exists() {
        let state = ProjectState {
            has_jest_config: true,
            has_package_json: true,
            jest_in_pkg_json: true,
            ..Default::default()
        };
        let env = ProjectEnv::Node {
            has_typescript: true,
        };

        let input = r#"{"name": "test", "jest": {"preset": "ts-jest"}}"#;
        let output = normalize_package_json(input, &state, &env);

        assert!(!output.contains("\"jest\": {"));
    }
}
