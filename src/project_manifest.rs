//! ProjectManifest v8.2 — وثيقة المشروع الحية
//! تُبنى بعد كل Task ناجح — تُقرأ قبل كل Task
//! الهدف: حل Stateless problem في Plan mode

use std::collections::HashMap;
use std::path::Path;

// ─── الأنواع الأساسية ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FileManifest {
    pub purpose: String,
    pub symbols: Vec<SymbolInfo>,
}

#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Function,
    Class,
    Constant,
    Other,
}

impl SymbolKind {
    fn label(&self) -> &str {
        match self {
            SymbolKind::Function  => "fn",
            SymbolKind::Class     => "class",
            SymbolKind::Constant  => "const",
            SymbolKind::Other     => "def",
        }
    }
}

// ─── ProjectManifest ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ProjectManifest {
    pub language: String,
    pub files: HashMap<String, FileManifest>,
    pub completed_tasks: Vec<String>,
}

impl ProjectManifest {
    /// بناء Manifest من ملفات الـ workspace الحالية
    pub fn build_from_workspace(workspace: &Path) -> Self {
        let mut manifest = ProjectManifest::default();

        let entries = match std::fs::read_dir(workspace) {
            Ok(e) => e,
            Err(_) => return manifest,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() { continue; }

            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            // تجاهل ملفات الاختبار والـ venv
            if name.starts_with("test_") || name.starts_with('.') {
                continue;
            }
            if path.to_string_lossy().contains("venv/") {
                continue;
            }

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let symbols = extract_symbols(&content, ext);
            if symbols.is_empty() && !["py","ts","js","rs","go"].contains(&ext) {
                continue;
            }

            // تحديد اللغة
            if manifest.language.is_empty() {
                manifest.language = match ext {
                    "py" => "python".into(),
                    "ts" => "typescript".into(),
                    "js" => "javascript".into(),
                    "rs" => "rust".into(),
                    "go" => "go".into(),
                    _ => String::new(),
                };
            }

            let purpose = infer_purpose(&name, &content);
            manifest.files.insert(name, FileManifest { purpose, symbols });
        }

        manifest
    }

    /// حجم الـ Manifest بالتوكنات (تقريبي)
    pub fn estimated_tokens(&self) -> usize {
        self.to_context_prompt().len() / 4
    }

    /// تحويل الـ Manifest إلى نص يُحقن في البرومبت
    pub fn to_context_prompt(&self) -> String {
        if self.files.is_empty() {
            return String::new();
        }

        let mut ctx = String::new();
        ctx.push_str("=== PROJECT MANIFEST ===\n");

        if !self.language.is_empty() {
            ctx.push_str(&format!("Language: {}\n\n", self.language));
        }

        // الملفات والرموز
        ctx.push_str("FILES:\n");
        let mut sorted_files: Vec<_> = self.files.iter().collect();
        sorted_files.sort_by_key(|(k, _)| k.as_str());

        for (filename, fm) in &sorted_files {
            ctx.push_str(&format!("  {} — {}\n", filename, fm.purpose));
            if !fm.symbols.is_empty() {
                let sym_names: Vec<_> = fm.symbols.iter()
                    .map(|s| s.name.as_str())
                    .collect();
                ctx.push_str(&format!("    symbols: {}\n", sym_names.join(", ")));
            }
        }

        // الرموز المتاحة للاستيراد
        let importable = self.importable_symbols();
        if !importable.is_empty() {
            ctx.push_str("\nAVAILABLE SYMBOLS (import — do NOT redefine):\n");
            for (sym, file) in &importable {
                ctx.push_str(&format!("  {} → {}\n", sym, file));
            }
        }

        // المهام المكتملة
        if !self.completed_tasks.is_empty() {
            ctx.push_str("\nCOMPLETED:\n");
            for task in &self.completed_tasks {
                ctx.push_str(&format!("  ✅ {}\n", task));
            }
        }

        // القواعد
        ctx.push_str("\nRULES:\n");
        ctx.push_str("  - patch_file for ALL existing files above\n");
        ctx.push_str("  - import symbols from their files — NEVER redefine\n");
        ctx.push_str("  - NEVER recreate files already listed above\n");
        ctx.push_str("=== END MANIFEST ===\n\n");

        ctx
    }

    /// كل الرموز القابلة للاستيراد: symbol_name → filename
    fn importable_symbols(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        let mut sorted_files: Vec<_> = self.files.iter().collect();
        sorted_files.sort_by_key(|(k, _)| k.as_str());

        for (filename, fm) in &sorted_files {
            for sym in &fm.symbols {
                // فقط functions و classes — ليس constants صغيرة
                if matches!(sym.kind, SymbolKind::Function | SymbolKind::Class) {
                    result.push((
                        format!("{} {}", sym.kind.label(), sym.name),
                        filename.to_string(),
                    ));
                }
            }
        }
        result
    }
}

// ─── استخراج الرموز حسب اللغة ─────────────────────────────────────────────────

fn extract_symbols(content: &str, ext: &str) -> Vec<SymbolInfo> {
    match ext {
        "py" => extract_python_symbols(content),
        "ts" | "js" => extract_ts_symbols(content),
        "rs" => extract_rust_symbols(content),
        "go" => extract_go_symbols(content),
        _ => Vec::new(),
    }
}

fn extract_python_symbols(content: &str) -> Vec<SymbolInfo> {
    let mut symbols = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("def ") {
            let name = trimmed[4..].split('(').next().unwrap_or("").trim();
            if !name.is_empty() && !name.starts_with('_') {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Function,
                    signature: trimmed.trim_end_matches(':').to_string(),
                });
            }
        } else if trimmed.starts_with("class ") {
            let name = trimmed[6..].split(['(', ':']).next().unwrap_or("").trim();
            if !name.is_empty() {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Class,
                    signature: trimmed.trim_end_matches(':').to_string(),
                });
            }
        } else if trimmed.contains(" = ") && !trimmed.starts_with(' ') {
            // متغيرات المستوى الأعلى فقط
            let name = trimmed.split('=').next().unwrap_or("").trim();
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Constant,
                    signature: name.to_string(),
                });
            }
        }
    }
    symbols
}

fn extract_ts_symbols(content: &str) -> Vec<SymbolInfo> {
    let mut symbols = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("export function ") || trimmed.starts_with("export async function ") {
            let after = trimmed.split("function ").nth(1).unwrap_or("");
            let name = after.split('(').next().unwrap_or("").trim();
            if !name.is_empty() {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Function,
                    signature: format!("function {}", name),
                });
            }
        } else if trimmed.starts_with("export class ") {
            let name = trimmed[13..].split([' ', '{']).next().unwrap_or("").trim();
            if !name.is_empty() {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Class,
                    signature: format!("class {}", name),
                });
            }
        } else if trimmed.starts_with("export const ") {
            let name = trimmed[13..].split([' ', '=']).next().unwrap_or("").trim();
            if !name.is_empty() {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Constant,
                    signature: format!("const {}", name),
                });
            }
        }
    }
    symbols
}

fn extract_rust_symbols(content: &str) -> Vec<SymbolInfo> {
    let mut symbols = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub fn ") {
            let name = trimmed[7..].split('(').next().unwrap_or("").trim();
            if !name.is_empty() {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Function,
                    signature: format!("pub fn {}", name),
                });
            }
        } else if trimmed.starts_with("pub struct ") {
            let name = trimmed[11..].split([' ', '<', '{']).next().unwrap_or("").trim();
            if !name.is_empty() {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Class,
                    signature: format!("pub struct {}", name),
                });
            }
        } else if trimmed.starts_with("pub enum ") {
            let name = trimmed[9..].split([' ', '<', '{']).next().unwrap_or("").trim();
            if !name.is_empty() {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Class,
                    signature: format!("pub enum {}", name),
                });
            }
        }
    }
    symbols
}

fn extract_go_symbols(content: &str) -> Vec<SymbolInfo> {
    let mut symbols = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("func ") {
            let name = trimmed[5..].split('(').next().unwrap_or("").trim();
            if !name.is_empty() {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Function,
                    signature: format!("func {}", name),
                });
            }
        } else if trimmed.starts_with("type ") && trimmed.contains(" struct") {
            let name = trimmed[5..].split_whitespace().next().unwrap_or("").trim();
            if !name.is_empty() {
                symbols.push(SymbolInfo {
                    name: name.to_string(),
                    kind: SymbolKind::Class,
                    signature: format!("type {} struct", name),
                });
            }
        }
    }
    symbols
}

// ─── تخمين غرض الملف من اسمه ومحتواه ────────────────────────────────────────

fn infer_purpose(name: &str, content: &str) -> String {
    let name_lower = name.to_lowercase();

    if name_lower.contains("model")  { return "database models".into(); }
    if name_lower.contains("schema") { return "data schemas".into(); }
    if name_lower.contains("auth")   { return "authentication".into(); }
    if name_lower.contains("database") || name_lower.contains("db") {
        return "database connection".into();
    }
    if name_lower == "main.py" || name_lower == "main.ts" || name_lower == "main.rs" {
        return "application entry point".into();
    }
    if name_lower.contains("route")  { return "API routes".into(); }
    if name_lower.contains("util")   { return "utilities".into(); }
    if name_lower.contains("config") { return "configuration".into(); }
    if name_lower.contains("order")  { return "orders logic".into(); }
    if name_lower.contains("product") { return "products logic".into(); }
    if name_lower.contains("user")   { return "user logic".into(); }

    // تخمين من المحتوى
    if content.contains("FastAPI") || content.contains("@app.") {
        return "FastAPI application".into();
    }
    if content.contains("Base") && content.contains("Column") {
        return "SQLAlchemy models".into();
    }
    if content.contains("create_engine") || content.contains("SessionLocal") {
        return "database session".into();
    }

    "module".into()
}

// ─── Unit Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_symbols_extraction() {
        let code = r#"
from sqlalchemy import Column
class User(Base):
    id = Column(Integer)
class Product(Base):
    name = Column(String)
def get_db():
    yield db
def create_token(data: dict):
    return jwt.encode(data)
"#;
        let symbols = extract_python_symbols(code);
        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"User"),    "missing User class");
        assert!(names.contains(&"Product"), "missing Product class");
        assert!(names.contains(&"get_db"),  "missing get_db function");
        assert!(names.contains(&"create_token"), "missing create_token");
    }

    #[test]
    fn test_manifest_build_empty_dir() {
        let tmp = std::env::temp_dir().join("sel_manifest_test_empty");
        let _ = std::fs::create_dir_all(&tmp);
        let manifest = ProjectManifest::build_from_workspace(&tmp);
        assert!(manifest.files.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_manifest_to_context_prompt() {
        let mut manifest = ProjectManifest::default();
        manifest.language = "python".into();
        manifest.files.insert("database.py".into(), FileManifest {
            purpose: "database connection".into(),
            symbols: vec![
                SymbolInfo {
                    name: "get_db".into(),
                    kind: SymbolKind::Function,
                    signature: "def get_db():".into(),
                },
                SymbolInfo {
                    name: "SessionLocal".into(),
                    kind: SymbolKind::Constant,
                    signature: "SessionLocal".into(),
                },
            ],
        });
        manifest.files.insert("models.py".into(), FileManifest {
            purpose: "database models".into(),
            symbols: vec![
                SymbolInfo {
                    name: "User".into(),
                    kind: SymbolKind::Class,
                    signature: "class User(Base)".into(),
                },
            ],
        });
        manifest.completed_tasks.push("Task 1: Create models".into());

        let prompt = manifest.to_context_prompt();

        assert!(prompt.contains("PROJECT MANIFEST"),  "missing header");
        assert!(prompt.contains("database.py"),        "missing database.py");
        assert!(prompt.contains("models.py"),          "missing models.py");
        assert!(prompt.contains("get_db"),             "missing get_db symbol");
        assert!(prompt.contains("User"),               "missing User symbol");
        assert!(prompt.contains("COMPLETED"),          "missing completed section");
        assert!(prompt.contains("Task 1: Create models"), "missing task");
        assert!(prompt.contains("RULES"),              "missing rules");
        assert!(prompt.contains("patch_file"),         "missing patch_file rule");

        // تحقق من الحجم
        let tokens = prompt.len() / 4;
        assert!(tokens < 300, "Manifest too large: {} tokens (max 300)", tokens);
    }

    #[test]
    fn test_infer_purpose() {
        assert_eq!(infer_purpose("models.py", ""), "database models");
        assert_eq!(infer_purpose("database.py", ""), "database connection");
        assert_eq!(infer_purpose("auth.py", ""), "authentication");
        assert_eq!(infer_purpose("main.py", ""), "application entry point");
        assert_eq!(infer_purpose("schemas.py", ""), "data schemas");
    }

    #[test]
    fn test_ts_symbols_extraction() {
        let code = r#"
export function createUser(data: UserDto) {}
export class AuthService {}
export const SECRET_KEY = 'abc';
"#;
        let symbols = extract_ts_symbols(code);
        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"createUser"));
        assert!(names.contains(&"AuthService"));
        assert!(names.contains(&"SECRET_KEY"));
    }
}
