// src/decision.rs — v1.0: Decision & Validation Logic
// منطق التحقق وبناء السياق — مستخرج من agent.rs لتقليل التعقيد

use std::path::Path;
use crate::protocol::Cmd;
use crate::types::ContextConfig;

// ══════════════════════════════════════════════════════════
// Goal Validator v1.2
// ══════════════════════════════════════════════════════════

/// يتحقق من أن الهدف محدد بما يكفي للتنفيذ
pub fn validate_goal(goal: &str) -> Option<String> {
    let g = goal.to_lowercase();
    let len = goal.trim().len();
    if len < 10 {
        return Some("Goal too short.".to_string());
    }
    let real_keywords = [
        "fix",
        "implement",
        "refactor",
        "update",
        "migrate",
        "failing",
        "crate",
        "existing",
        "workspace",
    ];
    if real_keywords.iter().any(|kw| g.contains(kw)) {
        return None;
    }
    let has_test = g.contains("test")
        || g.contains("pytest")
        || g.contains("assert")
        || g.contains("spec")
        || g.contains("verify");
    if !has_test {
        return Some("Goal has no test requirement — add tests to verify.".to_string());
    }
    let vague = (g.contains("test") || g.contains("assert"))
        && (g.contains("some value") || g.contains("correct value"));
    if vague {
        return Some("Ambiguous values — specify exact expected values.".to_string());
    }
    None
}

// ══════════════════════════════════════════════════════════
// Patch Uniqueness Validator v5.6
// ══════════════════════════════════════════════════════════

/// يتحقق من أن كل search block في patch_file فريد في الملف المستهدف
pub fn validate_patch_uniqueness(workspace: &Path, plan: &[Cmd]) -> Vec<String> {
    let mut issues = Vec::new();
    for cmd in plan {
        if let Cmd::PatchFile { path, search, .. } = cmd {
            let full_path = workspace.join(path);
            if !full_path.exists() {
                continue;
            }
            let content = match std::fs::read_to_string(&full_path) {
                Ok(c) => c,
                Err(e) => {
                    issues.push(format!("Could not read '{}': {}", path, e));
                    continue;
                }
            };
            let count = content.matches(search.as_str()).count();
            if count == 0 {
                issues.push(format!(
                    "search block not found in '{}' — copy text VERBATIM from the file",
                    path
                ));
            } else if count > 1 {
                issues.push(format!(
                    "search block found {} times in '{}' — add more surrounding context lines",
                    count, path
                ));
            }
        }
    }
    issues
}

// ══════════════════════════════════════════════════════════
// Language Hint Builder v5.6
// ══════════════════════════════════════════════════════════

/// يبني تلميح اللغة بناءً على manifest الـ workspace
pub fn build_lang_hint(workspace: &Path) -> String {
    if workspace.join("Cargo.toml").exists() {
        "\nCRITICAL: This is a RUST project (Cargo.toml exists). Write ONLY Rust code. Do NOT create Python or JS files.".to_string()
    } else if workspace.join("package.json").exists() {
        "\nCRITICAL: This is a Node.js project (package.json exists). Write ONLY JS/TS code."
            .to_string()
    } else if workspace.join("go.mod").exists() {
        "\nCRITICAL: This is a Go project (go.mod exists). Write ONLY Go code.".to_string()
    } else {
        String::new()
    }
}

// ══════════════════════════════════════════════════════════
// Skeleton Context Builder v5.6
// ══════════════════════════════════════════════════════════

/// يبني سياق هيكلي يعرض البنية الحالية للملفات (pub struct, pub fn, ...)
pub fn build_skeleton_context(workspace: &Path) -> String {
    let mut map = String::new();
    // v5.8.1: أضف محتوى Cargo.toml دائماً في Planning
    if let Ok(toml) = std::fs::read_to_string(workspace.join("Cargo.toml")) {
        map.push_str(&format!(
            "CURRENT Cargo.toml CONTENT (use patch_file with EXACT text):\n```\n{}\n```\n\n",
            toml.trim()
        ));
    }
    // v5.8.2: أضف محتوى src/lib.rs دائماً في Planning
    if let Ok(lib) = std::fs::read_to_string(workspace.join("src/lib.rs")) {
        map.push_str(&format!(
            "CURRENT src/lib.rs CONTENT (use patch_file with EXACT text):\n```\n{}\n```\n\n",
            lib.trim()
        ));
    }
    if let Ok(toml) = std::fs::read_to_string(workspace.join("Cargo.toml")) {
        if let Some(name) = toml
            .lines()
            .find(|l| l.trim().starts_with("name"))
            .and_then(|l| l.split('"').nth(1))
        {
            map.push_str(&format!("CRATE NAME: {}\n", name));
            map.push_str(&format!("TEST IMPORT: use {}::\n\n", name));
        }
    }
    let src_dir = workspace.join("src");
    if src_dir.exists() {
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&src_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "rs").unwrap_or(false))
            .collect();
        files.sort();
        for path in files {
            let rel = path
                .strip_prefix(workspace)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            if let Ok(src) = std::fs::read_to_string(&path) {
                let skeleton: Vec<String> = src
                    .lines()
                    .filter(|l| {
                        let t = l.trim();
                        t.starts_with("pub struct ")
                            || t.starts_with("pub enum ")
                            || t.starts_with("pub fn ")
                            || t.starts_with("fn ")
                            || t.starts_with("pub mod ")
                            || t.starts_with("mod ")
                            || t.starts_with("pub use ")
                            || t.starts_with("impl ")
                    })
                    .map(|l| {
                        let t = l.trim();
                        let sig = if t.contains('{') {
                            t.split('{').next().unwrap_or(t).trim().to_string() + " { ... }"
                        } else {
                            t.to_string()
                        };
                        format!("  {}", sig)
                    })
                    .collect();
                if !skeleton.is_empty() {
                    map.push_str(&format!("FILE: {}\n{}\n\n", rel, skeleton.join("\n")));
                }
            }
        }
    }
    if !map.is_empty() {
        map.push_str("CRITICAL RULES (violations = build failure):\n");
        map.push_str("- NEVER use write_file on existing files — use patch_file only\n");
        map.push_str("- NEVER redefine functions already listed above\n");
        map.push_str(
            "- NEVER guess the crate name — use exactly what CRATE NAME shows above\n",
        );
    }
    map
}

// ══════════════════════════════════════════════════════════
// Workspace Context Builder v6.6
// ══════════════════════════════════════════════════════════

/// يقرأ كل ملفات الـ workspace الموجودة ويبني سياق كامل
pub fn build_workspace_context(workspace: &Path) -> String {
    let mut ctx = String::new();

    // الامتدادات المدعومة
    let supported = ["ts", "js", "py", "go", "rs", "toml", "json", "mod"];

    // اقرأ كل الملفات بشكل recursive (حد 50 ملف، حد 300 سطر لكل ملف)
    let mut files: Vec<std::path::PathBuf> = walkdir::WalkDir::new(workspace)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.path().to_path_buf())
        .filter(|p| p.is_file())
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            supported.contains(&ext)
        })
        .filter(|p| {
            // تجاهل node_modules, venv, dist, target
            let s = p.to_string_lossy();
            !s.contains("node_modules")
                && !s.contains("/venv/")
                && !s.contains("/dist/")
                && !s.contains("/target/")
                && !s.contains("/.")
                && !s.contains("package-lock")
        })
        .take(50)
        .collect();
    files.sort();

    if files.is_empty() {
        return String::new();
    }

    ctx.push_str("=== EXISTING WORKSPACE FILES (read carefully before planning) ===\n");
    ctx.push_str("CRITICAL: Use patch_file (NOT write_file) for ALL files listed below.\n\n");

    // سقف صارم: 4000 token إجمالي للـ context (حوالي 16000 حرف)
    const MAX_CONTEXT_CHARS: usize = 16_000;
    let mut total_chars = 0usize;

    for path in &files {
        if total_chars >= MAX_CONTEXT_CHARS {
            ctx.push_str("... (remaining files omitted — context limit reached)\n");
            break;
        }
        let rel = path.strip_prefix(workspace).unwrap_or(path).to_string_lossy();
        if let Ok(src) = std::fs::read_to_string(path) {
            let lines: Vec<&str> = src.lines().collect();
            // حد 60 سطر لكل ملف بدل 300
            let max_lines = 60usize;
            let preview: Vec<&str> = lines.iter().take(max_lines).cloned().collect();
            let file_content = format!(
                "--- FILE: {} ({} lines) ---\n{}\n{}\n",
                rel,
                lines.len(),
                preview.join("\n"),
                if lines.len() > max_lines {
                    format!("... ({} more lines)", lines.len() - max_lines)
                } else {
                    String::new()
                }
            );
            // لا تضف إذا سيتجاوز الحد
            if total_chars + file_content.len() > MAX_CONTEXT_CHARS {
                ctx.push_str(&format!(
                    "--- FILE: {} (skipped — context limit) ---\n\n",
                    rel
                ));
                break;
            }
            total_chars += file_content.len();
            ctx.push_str(&file_content);
        }
    }

    ctx.push_str("=== END OF EXISTING FILES ===\n\n");
    ctx
}

// ══════════════════════════════════════════════════════════
// Reference File Context Builder v5.1
// ══════════════════════════════════════════════════════════

/// يبني سياق من الملف المرجعي إذا كان موجوداً
pub fn build_ref_context(config: &ContextConfig) -> String {
    if let Some(ref ref_path) = config.ref_file {
        crate::context::read_ref_file(ref_path)
            .map(|s| format!("\nREFERENCE FILE (use exact signatures):\n{}\n", s))
            .unwrap_or_default()
    } else {
        String::new()
    }
}
