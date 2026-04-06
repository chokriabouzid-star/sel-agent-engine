use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// وصف ملف واحد في المشروع
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileManifest {
    pub owns:       Vec<String>,
    pub import_as:  HashMap<String, String>,
    pub locked:     bool,
    pub language:   String,
    pub signatures: HashMap<String, String>,
}

/// Manifest المشروع كاملاً
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectManifest {
    pub language:   String,
    pub files:      HashMap<String, FileManifest>,
    pub task_count: usize,
}

impl ProjectManifest {
    pub fn new(language: &str) -> Self {
        Self {
            language:   language.to_string(),
            files:      HashMap::new(),
            task_count: 0,
        }
    }

    pub fn save(&self, workspace: &Path) -> anyhow::Result<()> {
        let sel_dir = workspace.join(".sel");
        std::fs::create_dir_all(&sel_dir)?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(sel_dir.join("manifest.json"), json)?;
        Ok(())
    }

    pub fn load(workspace: &Path) -> anyhow::Result<Self> {
        let path = workspace.join(".sel/manifest.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn register_file(
        &mut self,
        file_name: &str,
        owns:      Vec<String>,
        language:  &str,
        locked:    bool,
    ) {
        let import_as  = generate_import_map(file_name, &owns, language);
        let signatures = HashMap::new();
        self.files.insert(
            file_name.to_string(),
            FileManifest { owns, import_as, locked, language: language.to_string(), signatures },
        );
    }

    /// قسم السياق للـ prompt — مضغوط وواضح
    pub fn to_context_section(&self) -> String {
        if self.files.is_empty() {
            return String::new();
        }

        let mut s = String::new();
        s.push_str("=== LOCKED INTERFACES — USE EXACT IMPORTS BELOW ===\n");
        s.push_str("CRITICAL: These files are LOCKED. Do NOT rewrite them.\n");
        s.push_str("Use ONLY the import lines shown. Do NOT guess import paths.\n\n");

        // رتّب الملفات بشكل ثابت
        let mut file_entries: Vec<(&String, &FileManifest)> =
            self.files.iter().collect();
        file_entries.sort_by_key(|(k, _)| k.as_str());

        for (file, manifest) in &file_entries {
            if manifest.owns.is_empty() {
                continue;
            }

            s.push_str(&format!(
                "FILE: {} (language={}, locked={})\n",
                file, manifest.language, manifest.locked
            ));
            s.push_str(&format!(
                "  Defines: {}\n",
                manifest.owns.join(", ")
            ));
            s.push_str("  Import lines (copy exactly):\n");

            let mut symbols: Vec<(&String, &String)> =
                manifest.import_as.iter().collect();
            symbols.sort_by_key(|(k, _)| k.as_str());

            for (sym, import_line) in &symbols {
                s.push_str(&format!("    {}\n", import_line));
                if let Some(sig) = manifest.signatures.get(*sym) {
                    s.push_str(&format!("    # {}\n", sig));
                }
            }
            s.push('\n');
        }

        s.push_str("=== END LOCKED INTERFACES ===\n\n");
        s
    }
}

fn generate_import_map(
    file_name: &str,
    symbols:   &[String],
    language:  &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let stem = Path::new(file_name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // مسار الاستيراد: app/models.py → app.models
    let import_path = file_name
        .trim_end_matches(".py")
        .trim_end_matches(".ts")
        .trim_end_matches(".js")
        .replace('/', ".")
        .replace('\\', ".");

    for symbol in symbols {
        let import_line = match language {
            "python" => {
                if import_path.contains('.') {
                    format!("from {} import {}", import_path, symbol)
                } else {
                    format!("from {} import {}", stem, symbol)
                }
            }
            "typescript" | "javascript" => {
                format!("import {{ {} }} from './{}';", symbol, stem)
            }
            "rust" => format!("use crate::{}::{};", stem, symbol),
            "go"   => format!("// use package {}, symbol {}", stem, symbol),
            _      => format!("from {} import {}", stem, symbol),
        };
        map.insert(symbol.clone(), import_line);
    }
    map
}

/// استخراج رموز من أي ملف مدعوم — بدون استدعاء Python خارجي
pub fn extract_symbols_rust(file_path: &Path) -> Vec<String> {
    let content = match std::fs::read_to_string(file_path) {
        Ok(c)  => c,
        Err(_) => return vec![],
    };
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "py"        => extract_python_symbols_inline(&content),
        "ts" | "js" => extract_ts_symbols_inline(&content),
        "go"        => extract_go_symbols_inline(&content),
        "rs"        => extract_rust_symbols_inline(&content),
        _           => vec![],
    }
}

fn extract_name_after(line: &str, prefix: &str) -> Option<String> {
    let pos  = line.find(prefix)?;
    let rest = &line[pos + prefix.len()..];
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() { None } else { Some(name) }
}

fn extract_python_symbols_inline(content: &str) -> Vec<String> {
    let mut symbols = vec![];
    let mut seen    = std::collections::HashSet::new();

    for line in content.lines() {
        let t = line.trim();
        let name = if t.starts_with("async def ") {
            extract_name_after(t, "async def ")
        } else if t.starts_with("def ") {
            extract_name_after(t, "def ")
        } else if t.starts_with("class ") {
            extract_name_after(t, "class ")
        } else if !t.starts_with(' ')
            && !t.starts_with('#')
            && !t.starts_with('"')
            && !t.starts_with('\'')
            && t.contains(" = ")
        {
            let name = t.split(" = ").next().unwrap_or("").trim().to_string();
            if !name.is_empty()
                && !name.starts_with('_')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                Some(name)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(n) = name {
            if !n.starts_with('_') && seen.insert(n.clone()) {
                symbols.push(n);
            }
        }
    }
    symbols
}

fn extract_ts_symbols_inline(content: &str) -> Vec<String> {
    let mut symbols = vec![];
    let mut seen    = std::collections::HashSet::new();

    for line in content.lines() {
        let t = line.trim();
        let name = if t.starts_with("export async function ") {
            extract_name_after(t, "export async function ")
        } else if t.starts_with("export function ") {
            extract_name_after(t, "export function ")
        } else if t.starts_with("export class ") {
            extract_name_after(t, "export class ")
        } else if t.starts_with("export const ") {
            extract_name_after(t, "export const ")
        } else if t.starts_with("export interface ") {
            extract_name_after(t, "export interface ")
        } else if t.starts_with("export type ") {
            extract_name_after(t, "export type ")
        } else {
            None
        };

        if let Some(n) = name {
            if seen.insert(n.clone()) {
                symbols.push(n);
            }
        }
    }
    symbols
}

fn extract_go_symbols_inline(content: &str) -> Vec<String> {
    let mut symbols = vec![];
    let mut seen    = std::collections::HashSet::new();

    for line in content.lines() {
        let t = line.trim();
        let name = if t.starts_with("func ") && !t.contains("(r ") && !t.contains("(s ") {
            extract_name_after(t, "func ")
        } else if t.starts_with("type ") && t.contains(" struct") {
            extract_name_after(t, "type ")
        } else {
            None
        };

        if let Some(n) = name {
            if n.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                if seen.insert(n.clone()) {
                    symbols.push(n);
                }
            }
        }
    }
    symbols
}

fn extract_rust_symbols_inline(content: &str) -> Vec<String> {
    let mut symbols = vec![];
    let mut seen    = std::collections::HashSet::new();

    for line in content.lines() {
        let t = line.trim();
        let name = if t.starts_with("pub async fn ") {
            extract_name_after(t, "pub async fn ")
        } else if t.starts_with("pub fn ") {
            extract_name_after(t, "pub fn ")
        } else if t.starts_with("pub struct ") {
            extract_name_after(t, "pub struct ")
        } else if t.starts_with("pub enum ") {
            extract_name_after(t, "pub enum ")
        } else if t.starts_with("pub trait ") {
            extract_name_after(t, "pub trait ")
        } else {
            None
        };

        if let Some(n) = name {
            if seen.insert(n.clone()) {
                symbols.push(n);
            }
        }
    }
    symbols
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_symbols() {
        let code = "engine = create_engine('sqlite:///db')\n\
                    SessionLocal = sessionmaker(bind=engine)\n\
                    \n\
                    def get_db():\n\
                        pass\n\
                    \n\
                    class User:\n\
                        pass\n";
        let syms = extract_python_symbols_inline(code);
        assert!(syms.contains(&"engine".to_string()),       "engine missing");
        assert!(syms.contains(&"SessionLocal".to_string()), "SessionLocal missing");
        assert!(syms.contains(&"get_db".to_string()),       "get_db missing");
        assert!(syms.contains(&"User".to_string()),         "User missing");
    }

    #[test]
    fn test_import_map_python_simple() {
        let map = generate_import_map(
            "database.py",
            &["get_db".to_string(), "engine".to_string()],
            "python",
        );
        assert_eq!(map["get_db"], "from database import get_db");
        assert_eq!(map["engine"], "from database import engine");
    }

    #[test]
    fn test_import_map_python_subdir() {
        let map = generate_import_map(
            "app/models.py",
            &["User".to_string()],
            "python",
        );
        assert_eq!(map["User"], "from app.models import User");
    }

    #[test]
    fn test_context_section_format() {
        let mut m = ProjectManifest::new("python");
        m.register_file(
            "models.py",
            vec!["User".to_string(), "Product".to_string()],
            "python",
            true,
        );
        m.register_file(
            "database.py",
            vec!["get_db".to_string(), "engine".to_string()],
            "python",
            true,
        );
        let ctx = m.to_context_section();
        assert!(ctx.contains("from database import get_db"));
        assert!(ctx.contains("from database import engine"));
        assert!(ctx.contains("from models import User"));
        assert!(ctx.contains("LOCKED"));
    }

    #[test]
    fn test_ts_symbols() {
        let code = "export function add(a: number): number { return a; }\n\
                    export class Stack<T> {}\n\
                    export const VERSION = '1.0';\n\
                    export interface Config {}\n";
        let syms = extract_ts_symbols_inline(code);
        assert!(syms.contains(&"add".to_string()));
        assert!(syms.contains(&"Stack".to_string()));
        assert!(syms.contains(&"VERSION".to_string()));
        assert!(syms.contains(&"Config".to_string()));
    }

    #[test]
    fn test_rust_symbols() {
        let code = "pub fn add(a: i32) -> i32 { a }\n\
                    pub struct Stack { items: Vec<i32> }\n\
                    pub enum Color { Red, Blue }\n";
        let syms = extract_rust_symbols_inline(code);
        assert!(syms.contains(&"add".to_string()));
        assert!(syms.contains(&"Stack".to_string()));
        assert!(syms.contains(&"Color".to_string()));
    }
}
