// src/repair_strategy.rs — v8.0: Directed Repair + Escalating Strategy

pub struct RepairCtx {
    pub source_files: Vec<String>,
    pub test_files: Vec<String>,
    pub source_file: String,
    pub function_name: String,
    pub prev_errors: Vec<String>,
}

impl RepairCtx {
    pub fn build(workspace: &std::path::Path, goal: &str, prev_error: Option<&String>) -> Self {
        let mut source_files = Vec::new();
        let mut test_files = Vec::new();

        let supported = ["ts", "js", "py", "go", "rs"];
        if let Ok(entries) = std::fs::read_dir(workspace) {
            for entry in entries.flatten() {
                if let Ok(ft) = entry.file_type() {
                    if ft.is_file() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let ext = std::path::Path::new(&name)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("");
                        if supported.contains(&ext) {
                            if name.contains("test")
                                || name.ends_with("_test.go")
                                || name.ends_with(".spec.ts")
                            {
                                test_files.push(name.clone());
                            } else {
                                source_files.push(name.clone());
                            }
                        }
                    }
                }
            }
        }

        let source_file = source_files
            .first()
            .cloned()
            .unwrap_or_else(|| "main".to_string());

        // Extract function name from goal heuristically
        let mut function_name = "the function".to_string();
        let tokens: Vec<&str> = goal.split_whitespace().collect();
        if let Some(idx) = tokens.iter().position(|&t| t == "Fix" || t == "implement") {
            if idx + 1 < tokens.len() {
                function_name = tokens[idx + 1]
                    .replace("(", "")
                    .replace(")", "")
                    .trim_end_matches('.')
                    .trim_end_matches(',')
                    .to_string();
            }
        }

        let prev_errors = prev_error.map(|s| vec![s.clone()]).unwrap_or_default();

        Self {
            source_files,
            test_files,
            source_file,
            function_name,
            prev_errors,
        }
    }
}

/// Read source file content from workspace, capped at max_bytes for prompt safety
fn read_source(workspace: &std::path::Path, filename: &str, max_bytes: usize) -> String {
    let path = workspace.join(filename);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            if content.len() > max_bytes {
                format!("{}... [truncated]", &content[..max_bytes])
            } else {
                content
            }
        }
        Err(_) => format!("[could not read {}]", filename),
    }
}

/// Build an escalating repair prompt based on attempt number.
/// Now includes actual source file content from disk so the LLM sees what it's fixing.
pub fn build_prompt(
    attempt: u8,
    error: &str,
    ctx: &RepairCtx,
    workspace: &std::path::Path,
) -> String {
    let is_loop = ctx.prev_errors.windows(2).any(|w| w[0] == w[1]) || attempt > 2;

    let source_content = read_source(workspace, &ctx.source_file, 600);
    let error_snippet = &error[..error.len().min(600)];

    let protected = if ctx.test_files.is_empty() {
        String::new()
    } else {
        format!("PROTECTED (do NOT modify): {}\n", ctx.test_files.join(", "))
    };

    match attempt {
        1 => format!(
            "REPAIR 1/N — STRATEGY: Minimal fix.\n\
             Fix SOURCE FILES only: {sources}\n\
             {protected}\
             CURRENT {file}:\n```\n{src}\n```\n\
             ERROR:\n```\n{err}\n```\n\
             Fix ONLY the failing line. Do NOT rewrite the whole file.",
            sources = ctx.source_files.join(", "),
            protected = protected,
            file = ctx.source_file,
            src = source_content,
            err = error_snippet,
        ),
        2 if is_loop => format!(
            "REPAIR 2/N — SAME ERROR REPEATED.\n\
             The minimal patch failed. Rewrite {file} completely from scratch.\n\
             Implement {func} correctly.\n\
             {protected}\
             ERROR:\n```\n{err}\n```",
            file = ctx.source_file,
            func = ctx.function_name,
            protected = protected,
            err = error_snippet,
        ),
        2 => format!(
            "REPAIR 2/N — STRATEGY: Rewrite the failing function.\n\
             The minimal fix failed. Rewrite {func} in {file} from scratch.\n\
             {protected}\
             CURRENT {file}:\n```\n{src}\n```\n\
             ERROR:\n```\n{err}\n```",
            func = ctx.function_name,
            file = ctx.source_file,
            protected = protected,
            src = source_content,
            err = error_snippet,
        ),
        _ => format!(
            "REPAIR {attempt}/N — FINAL ATTEMPT. Use the simplest possible algorithm.\n\
             ALL previous patches failed. REWRITE {file} completely.\n\
             Implement {func} using the most basic approach — ignore edge cases if needed.\n\
             {protected}\
             ERROR:\n```\n{err}\n```",
            attempt = attempt,
            file = ctx.source_file,
            func = ctx.function_name,
            protected = protected,
            err = error_snippet,
        ),
    }
}
