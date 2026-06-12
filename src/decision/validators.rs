// src/decision/validators.rs
use crate::protocol::Cmd;
use std::path::Path;

/// Validates that each search block in patch_file is unique in the target file
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

/// Validates plan integrity (no duplication, complete definitions)
pub fn validate_plan_integrity(plan: &[Cmd]) -> Vec<String> {
    let mut issues = Vec::new();
    let mut written_files = std::collections::HashSet::new();

    for cmd in plan {
        if let Cmd::WriteFile { path, .. } = cmd {
            if !written_files.insert(path.clone()) {
                issues.push(format!(
                    "PLAN ERROR: Duplicate write_file for '{}' in one plan. \
                     Combine into ONE write_file command.",
                    path
                ));
            }
        }
    }

    // Rust Completeness & Cargo Template Check v7.5
    let mut has_cargo_new = false;
    for cmd in plan {
        if let Cmd::Run { command } = cmd {
            if command.contains("cargo new") || command.contains("cargo init") {
                has_cargo_new = true;
            }
        }
    }

    for cmd in plan {
        match cmd {
            Cmd::WriteFile { path, content }
                if path.ends_with(".rs")
                    && (content.contains("#[cfg(test)]") || content.contains("mod tests"))
                    && content.contains("Stack::new()")
                    && !content.contains("struct Stack")
                    && !content.contains("use ") =>
            {
                issues.push(format!(
                    "COMPLETENESS ERROR in '{}': Test uses 'Stack' but \
                     'struct Stack' is not defined or imported.",
                    path
                ));
            }
            Cmd::PatchFile { path, .. }
                if has_cargo_new
                    && (path.ends_with("src/lib.rs")
                        || path.ends_with("src/main.rs")
                        || path.ends_with("Cargo.toml")) =>
            {
                issues.push(format!(
                    "PLAN ERROR: You used 'cargo new' which creates a dummy '{}'. \
                     You MUST use write_file to completely replace it, \
                     DO NOT use patch_file.",
                    path
                ));
            }
            _ => {}
        }
    }

    issues
}

/// Detect writes to protected manifest files that should be rejected during planning,
/// before execution wastes repair budget.
pub fn validate_protected_writes(plan: &[Cmd]) -> Vec<String> {
    let mut issues = Vec::new();

    for cmd in plan {
        match cmd {
            Cmd::WriteFile { path, .. } if path == "go.mod" || path.ends_with("/go.mod") => {
                issues.push(
                    "PLAN ERROR: write_file on 'go.mod' is forbidden \
                     (constitution: no-overwrite-go-mod). \
                     Use `run: go mod init <name>` instead."
                        .to_string(),
                );
            }
            _ => {}
        }
    }

    issues
}
