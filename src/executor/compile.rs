pub fn go_compile_check(workspace: &std::path::Path) -> Option<String> {
    if !workspace.join("go.mod").exists() {
        eprintln!("[TRACE] go_compile_check: skipped (no go.mod)");
        return None;
    }
    eprintln!("[TRACE] go_compile_check: running...");
    let out = std::process::Command::new("go")
        .args(["test", "-run=^$", "-count=1"])
        .current_dir(workspace)
        .output()
        .ok()?;
    if !out.status.success() {
        Some(String::from_utf8_lossy(&out.stderr).to_string())
    } else {
        None
    }
}

/// v7.9.6: Python syntax check  catches SyntaxError/IndentationError without LLM
/// Uses py_compile (stdlib)  works fully offline
pub fn python_syntax_check(file: &std::path::Path) -> Option<String> {
    if !file.exists() || !file.extension().map(|e| e == "py").unwrap_or(false) {
        return None;
    }
    let file_str = file.to_string_lossy();
    eprintln!("[TRACE] python_syntax_check: checking {}", file_str);

    // Use python3 -m py_compile which is stdlib (no pip needed)
    let out = std::process::Command::new("python3")
        .args(["-m", "py_compile", &file_str])
        .output()
        .ok()?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        // Filter to show only the relevant error lines
        let error_msg: String = stderr
            .lines()
            .filter(|l| {
                l.contains("SyntaxError")
                    || l.contains("IndentationError")
                    || l.contains("TabError")
                    || l.contains("File \"")
                    || l.trim().starts_with('^')
                    || l.contains("line ")
            })
            .collect::<Vec<_>>()
            .join("\n");
        if error_msg.is_empty() {
            Some(stderr)
        } else {
            Some(error_msg)
        }
    } else {
        None
    }
}

/// v7.9.9 P6: Semantic Guard for tests
pub fn guard_assertion_integrity(
    path: &str,
    before: &str,
    after: &str,
) -> std::result::Result<(), String> {
    let before_asserts = count_assertions(before);
    let after_asserts = count_assertions(after);

    if after_asserts < before_asserts {
        return Err(format!(
            "BLOCKED: ASSERTION GUARD: patch on '{}' would reduce assertion count from {} to {}  this weakens test quality. Fix the SOURCE code instead.",
            path, before_asserts, after_asserts
        ));
    }

    let before_vals = extract_expected_values(before);
    let after_vals = extract_expected_values(after);
    if !before_vals.is_subset(&after_vals) {
        return Err(format!(
            "BLOCKED: ASSERTION GUARD: patch on '{}' would change expected values (e.g. literals in assertions). This is FORBIDDEN. Fix the SOURCE code to match existing tests.",
            path
        ));
    }

    Ok(())
}

/// v7.9.9 P6: Count assertion statements in test file
/// Used by Semantic Guard to prevent test weakening
pub fn count_assertions(content: &str) -> usize {
    content
        .lines()
        .filter(|l| {
            let t = l.trim();
            // Python
            t.starts_with("assert ") || t.contains("assertEqual") || t.contains("assertRaises")
        // Go
        || t.contains("t.Errorf") || t.contains("t.Fatal") || t.contains("t.Error(")
        // JS/TS
        || t.contains("expect(") || t.contains("assert.") || t.contains("toBe(")
        // Rust
        || t.contains("assert_eq!") || t.contains("assert_ne!") || t.contains("assert!(")
        })
        .count()
}

/// v7.9.9 P6: Extract expected values from assertion strings
/// Regex-based heuristic to detect literal changes in tests
pub fn extract_expected_values(content: &str) -> std::collections::BTreeSet<String> {
    let mut values = std::collections::BTreeSet::new();
    // Extract all numbers and quoted strings as a heuristic for test literals
    let re = regex::Regex::new(r#""[^"]*"|'[^']*'|-?\b\d+\b"#).unwrap();
    for cap in re.captures_iter(content) {
        values.insert(cap[0].to_string());
    }
    values
}
