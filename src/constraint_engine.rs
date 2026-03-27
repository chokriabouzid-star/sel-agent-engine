//! SEL Agent v6.3 - Constraint Engine
//!
//! مسؤولياته:
//! 1. Environment Lock: منع pytest في Node، منع npm في Python
//! 2. Config Deduplication: منع كتابة jest.config.js إذا كان package.json يحتوي jest
//! 3. Dependency Normalization: استبدال versions خاطئة بـ pinned stacks
//! 4. Fatal Constraints: إيقاف الخطط الفاشلة تمامًا
//!
//! الترتيب مهم:
//! 1️⃣ environment::enforce → 2️⃣ config::deduplicate → 3️⃣ dependencies::normalize

use crate::protocol::Cmd;
use crate::scanner::has_ts_files;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ============================================================================
// أنواع البيانات الأساسية
// ============================================================================

/// بيئة المشروع - تُحسب مرة واحدة من Scanner
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectEnv {
    Node { has_typescript: bool },
    Python,
    Rust,
    Go,
    Unknown,
}

impl ProjectEnv {
    /// كشف البيئة من workspace
    pub fn detect(workspace: &Path) -> Self {
        if workspace.join("Cargo.toml").exists() {
            return ProjectEnv::Rust;
        }
        if workspace.join("go.mod").exists() {
            return ProjectEnv::Go;
        }
        if workspace.join("requirements.txt").exists()
            || workspace.join("pyproject.toml").exists()
        {
            return ProjectEnv::Python;
        }
        if workspace.join("package.json").exists() {
            let has_ts = has_ts_files(workspace);
            return ProjectEnv::Node { has_typescript: has_ts };
        }
        ProjectEnv::Unknown
    }
}

/// حالة المشروع على disk - ما هو موجود فعلاً
#[derive(Debug, Clone, Default)]
pub struct ProjectState {
    pub files: HashSet<String>,
    pub has_jest_config: bool,
    pub has_tsconfig: bool,
    pub has_package_json: bool,
    pub jest_in_pkg_json: bool,
}

impl ProjectState {
    /// مسح workspace لمعرفة الملفات الموجودة
    pub fn scan(workspace: &Path) -> Self {
        let mut files = HashSet::new();
        let has_jest_config;
        let has_tsconfig;
        let has_package_json;
        let jest_in_pkg_json;

        // فحص jest.config.js
        if workspace.join("jest.config.js").exists() {
            files.insert("jest.config.js".to_string());
            has_jest_config = true;
        } else if workspace.join("jest.config.ts").exists() {
            files.insert("jest.config.ts".to_string());
            has_jest_config = true;
        } else {
            has_jest_config = false;
        }

        // فحص tsconfig.json
        if workspace.join("tsconfig.json").exists() {
            files.insert("tsconfig.json".to_string());
            has_tsconfig = true;
        } else {
            has_tsconfig = false;
        }

        // فحص package.json ووجود jest field
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
        let content = std::fs::read_to_string(path).ok()?;
        if let Ok(json) = serde_json::from_str::<Value>(&content) {
            if json.get("jest").is_some() {
                return true;
            }
        }
        false
    }
}

/// نتيجة تطبيق القيود
#[derive(Debug, Clone)]
pub enum ConstraintResult {
    /// نجاح - الأوامر المعدلة (أو الأصلية)
    Ok(Vec<Cmd>),
    /// فشل قاتل - يجب إعادة التخطيط من الصفر
    Fatal(String),
}

// ============================================================================
// Pinned Stacks - النسخ المضمونة المتوافقة
// ============================================================================

/// Pinned Stack لـ TypeScript + Jest
const TS_JEST_STACK: &str = "typescript@5.3.3 ts-jest@29.1.1 jest@29.7.0 @types/jest@29.5.11";

/// قالب package.json الصحيح لـ TypeScript
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
// القاعدة 1: Environment Lock
// ============================================================================

/// منع أوامر خارج بيئة المشروع
fn enforce_environment(plan: Vec<Cmd>, env: &ProjectEnv) -> Vec<Cmd> {
    let mut filtered = Vec::new();

    for cmd in plan {
        match cmd {
            Cmd::Run { ref command } => {
                let cmd_lower = command.to_lowercase();
                let is_allowed = match env {
                    ProjectEnv::Node { .. } => {
                        // منع أوامر Python في Node
                        !(cmd_lower.contains("pytest")
                            || cmd_lower.contains("venv")
                            || cmd_lower.contains("pip")
                            || cmd_lower.contains("python"))
                    }
                    ProjectEnv::Python => {
                        // منع أوامر Node في Python
                        !(cmd_lower.contains("npm")
                            || cmd_lower.contains("npx")
                            || cmd_lower.contains("node"))
                    }
                    ProjectEnv::Rust => {
                        // منع npm و pip في Rust
                        !(cmd_lower.contains("npm")
                            || cmd_lower.contains("pip")
                            || cmd_lower.contains("pytest"))
                    }
                    ProjectEnv::Go => {
                        // منع npm و pip في Go
                        !(cmd_lower.contains("npm") || cmd_lower.contains("pip"))
                    }
                    ProjectEnv::Unknown => true,
                };

                if is_allowed {
                    filtered.push(cmd);
                } else {
                    eprintln!("🔒 Constraint: blocked '{}' (wrong environment)", command);
                }
            }
            _ => filtered.push(cmd),
        }
    }

    filtered
}

// ============================================================================
// القاعدة 2: Config Deduplication - منع jest.config.js
// ============================================================================

/// معالجة WriteFile لـ package.json و jest.config.js
fn intercept_write_file(
    path: &str,
    content: &str,
    state: &ProjectState,
    env: &ProjectEnv,
) -> Option<Cmd> {
    let path_lower = path.to_lowercase();

    // حالة 1: كتابة jest.config.js
    if path_lower.ends_with("jest.config.js") || path_lower.ends_with("jest.config.ts") {
        // التحقق: هل package.json موجود وفيه jest field؟
        if state.has_package_json && state.jest_in_pkg_json {
            eprintln!(
                "🔒 Constraint: blocked write '{}' (jest already in package.json)",
                path
            );
            return None; // تجاهل الكتابة تمامًا
        }

        // إذا لم يكن هناك package.json أو ليس فيه jest field، نحتاج إلى تعديل package.json
        if state.has_package_json {
            eprintln!("🔒 Constraint: converting jest.config.js → merge into package.json");
            // سنقوم بإضافة jest config إلى package.json بدلاً من إنشاء الملف
            // هذا يتم في normalize_package_json
            return None; // نمنع الكتابة، وnormalize_package_json سيتولى الباقي
        }
    }

    // حالة 2: كتابة package.json - نحتاج إلى تطبيع المحتوى
    if path_lower.ends_with("package.json") {
        return Some(Cmd::WriteFile {
            path: path.to_string(),
            content: normalize_package_json(content, state, env).to_string(),
        });
    }

    None
}

/// تطبيع package.json: إضافة pinned stack، إزالة jest field إذا كان هناك jest.config.js
fn normalize_package_json(content: &str, state: &ProjectState, env: &ProjectEnv) -> String {
    let mut json: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("⚠️ Constraint: invalid package.json, using template");
            return CORRECT_PACKAGE_JSON_TS.to_string();
        }
    };

    // فقط لـ TypeScript projects
    let is_ts_project = match env {
        ProjectEnv::Node { has_typescript } => *has_typescript,
        _ => false,
    };

    if !is_ts_project {
        return content.to_string();
    }

    // 1. تطبيق pinned stack على devDependencies
    let dev_deps = json
        .get_mut("devDependencies")
        .and_then(|d| d.as_object_mut());

    if let Some(deps) = dev_deps {
        // استبدال typescript بالنسخة المثبتة
        deps.insert("typescript".to_string(), Value::String("5.3.3".to_string()));
        deps.insert("ts-jest".to_string(), Value::String("29.1.1".to_string()));
        deps.insert("jest".to_string(), Value::String("29.7.0".to_string()));
        deps.insert(
            "@types/jest".to_string(),
            Value::String("29.5.11".to_string()),
        );
    } else {
        // إذا لم يكن devDependencies موجودًا، أضفه
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

    // 2. تطبيق scripts الصحيحة
    let scripts = json.get_mut("scripts").and_then(|s| s.as_object_mut());
    if let Some(scripts) = scripts {
        // فقط إذا لم يكن هناك script موجود أو كان خاطئًا
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

    // 3. إضافة jest config داخل package.json (إذا لم يكن هناك jest.config.js)
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
        // إذا كان هناك jest.config.js، نزيل jest field من package.json
        if json.get("jest").is_some() {
            eprintln!("🔒 Constraint: removing 'jest' field from package.json (jest.config.js exists)");
            json.as_object_mut().and_then(|obj| obj.remove("jest"));
        }
    }

    // 4. التأكد من عدم وجود 'type': 'module' (يسبب مشاكل مع jest)
    if let Some(r#type) = json.get("type") {
        if r#type == "module" {
            eprintln!("🔒 Constraint: removing 'type': 'module' from package.json");
            json.as_object_mut().and_then(|obj| obj.remove("type"));
        }
    }

    serde_json::to_string_pretty(&json).unwrap_or_else(|_| content.to_string())
}

// ============================================================================
// القاعدة 3: Dependency Normalization
// ============================================================================

/// تطبيع أوامر npm install لاستخدام pinned stacks
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
                    eprintln!("🔒 Constraint: normalizing npm install → using pinned stack");
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
// القاعدة 4: Fatal Constraints
// ============================================================================

/// فحص fatal errors - خطط فاشلة بالكامل
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

    // فحص: مشروع Python لكن الخطة تحتوي npm
    if let ProjectEnv::Python = env {
        if has_npm && !has_pip {
            return Some(format!(
                "Fatal: Python project but plan contains npm commands (has_npm={}, has_pip={})",
                has_npm, has_pip
            ));
        }
    }

    // فحص: مشروع Node لكن الخطة تحتوي pip
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
// الواجهة الرئيسية
// ============================================================================

/// نقطة الدخول الرئيسية لتطبيق القيود
pub fn apply(
    plan: Vec<Cmd>,
    env: &ProjectEnv,
    state: &ProjectState,
) -> ConstraintResult {
    // 1. فحص fatal أولاً
    if let Some(reason) = check_fatal(&plan, env) {
        return ConstraintResult::Fatal(reason);
    }

    // 2. Environment Lock
    let plan = enforce_environment(plan, env);

    // 3. Config Deduplication - معالجة WriteFile
    let mut processed = Vec::new();
    for cmd in plan {
        match cmd {
            Cmd::WriteFile { ref path, ref content } => {
                if let Some(new_cmd) = intercept_write_file(path, content, state, env) {
                    processed.push(new_cmd);
                }
                // إذا كانت intercept_write_file أعادت None، نتجاهل الأمر (نمنع الكتابة)
            }
            _ => processed.push(cmd),
        }
    }

    // 4. Dependency Normalization
    let plan = normalize_dependencies(processed, env);

    ConstraintResult::Ok(plan)
}

// ============================================================================
// اختبارات الوحدة
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_detect_rust() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(ProjectEnv::detect(dir.path()), ProjectEnv::Rust);
    }

    #[test]
    fn test_detect_python() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();
        assert_eq!(ProjectEnv::detect(dir.path()), ProjectEnv::Python);
    }

    #[test]
    fn test_detect_node() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
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
        let dir = tempdir().unwrap();
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

        assert!(!output.contains("\"jest\""));
    }
}
