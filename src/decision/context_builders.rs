// src/decision/context_builders.rs
use crate::types::ContextConfig;
use std::path::Path;

/// Builds a language hint based on the workspace manifest
pub fn build_lang_hint(workspace: &Path) -> String {
    if workspace.join("Cargo.toml").exists() {
        "\nCRITICAL: This is a RUST project (Cargo.toml exists). \
         Write ONLY Rust code. Do NOT create Python or JS files."
            .to_string()
    } else if workspace.join("package.json").exists() {
        "\nCRITICAL: This is a Node.js project (package.json exists). \
         Write ONLY JS/TS code."
            .to_string()
    } else if workspace.join("go.mod").exists() {
        "\nCRITICAL: This is a Go project (go.mod exists). Write ONLY Go code.".to_string()
    } else {
        String::new()
    }
}

/// Builds a skeleton context showing the current structure of files
pub fn build_skeleton_context(workspace: &Path) -> String {
    let mut map = String::new();

    if let Ok(toml) = std::fs::read_to_string(workspace.join("Cargo.toml")) {
        map.push_str(&format!(
            "CURRENT Cargo.toml CONTENT (use patch_file with EXACT text):\n```\n{}\n```\n\n",
            toml.trim()
        ));
    }
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
        map.push_str("- NEVER guess the crate name — use exactly what CRATE NAME shows above\n");
    }
    map
}

/// Reads all existing workspace files and builds a full context
pub fn build_workspace_context(workspace: &Path) -> String {
    let mut ctx = String::new();
    let supported = ["ts", "js", "py", "go", "rs", "toml", "json", "mod"];

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

    const MAX_CONTEXT_CHARS: usize = 16_000;
    let mut total_chars = 0usize;

    for path in &files {
        if total_chars >= MAX_CONTEXT_CHARS {
            ctx.push_str("... (remaining files omitted — context limit reached)\n");
            break;
        }
        let rel = path
            .strip_prefix(workspace)
            .unwrap_or(path)
            .to_string_lossy();
        if let Ok(src) = std::fs::read_to_string(path) {
            let lines: Vec<&str> = src.lines().collect();
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

/// Builds a context from the reference file if it exists
pub fn build_ref_context(config: &ContextConfig) -> String {
    if let Some(ref ref_path) = config.ref_file {
        crate::context::read_ref_file(ref_path)
            .map(|s| format!("\nREFERENCE FILE (use exact signatures):\n{}\n", s))
            .unwrap_or_default()
    } else {
        String::new()
    }
}
