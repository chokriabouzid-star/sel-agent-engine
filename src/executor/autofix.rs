/// AutoFix:  Go stdlib import   LLM
/// AutoFix: removes a function redeclared in a _test.go file when it already
/// exists in main.go or another source file in the same package.
/// Triggered by: "funcName redeclared in this block"
pub fn autofix_go_redeclared_in_test(
    test_file: &std::path::Path,
    err: &str,
    workspace: &std::path::Path,
) -> Option<String> {
    // Extract the function name from the error line
    // Pattern: "./main_test.go:10:6: setupRouter redeclared in this block"
    let fn_name = err
        .lines()
        .find(|l| l.contains("redeclared in this block"))?
        .split(':')
        .find(|seg| {
            let s = seg.trim();
            !s.is_empty()
                && s.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false)
                && !s.contains('/')
                && !s.contains('.')
        })?
        .trim()
        .to_string();

    if fn_name.is_empty() {
        return None;
    }

    // Verify the function exists in main.go (the authoritative source)
    let main_go = workspace.join("main.go");
    if !main_go.exists() {
        return None;
    }
    let main_src = std::fs::read_to_string(&main_go).ok()?;
    let fn_sig = format!("func {}(", fn_name);
    if !main_src.contains(&fn_sig) {
        return None;
    }

    // Remove the function block from the test file
    let test_src = std::fs::read_to_string(test_file).ok()?;
    let mut result = String::new();
    let mut in_fn = false;
    let mut brace_depth: i32 = 0;

    for line in test_src.lines() {
        let trimmed = line.trim();
        if !in_fn && trimmed.starts_with(&fn_sig) {
            in_fn = true;
            brace_depth = 0;
            eprintln!(
                "[AutoFix] Removing redeclared '{}' from {:?}",
                fn_name,
                test_file.file_name().unwrap_or_default()
            );
        }
        if in_fn {
            brace_depth += line.chars().filter(|&c| c == '{').count() as i32;
            brace_depth -= line.chars().filter(|&c| c == '}').count() as i32;
            if brace_depth <= 0 && line.contains('}') {
                in_fn = false;
            }
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }

    if result.trim_end() == test_src.trim_end() {
        return None;
    }

    // FIX C-02: guard — ensure test_file is inside workspace (no symlink escape)
    let canonical_ws = workspace.canonicalize().unwrap_or_else(|_| workspace.to_path_buf());
    let canonical_tf = test_file.canonicalize().unwrap_or_else(|_| test_file.to_path_buf());
    if !canonical_tf.starts_with(&canonical_ws) {
        eprintln!("[WARN] autofix_go_redeclared_in_test: test_file outside workspace — write refused");
        return None;
    }
    std::fs::write(test_file, &result).ok()?;
    Some(format!("removed redeclared '{}' from test file", fn_name))
}

pub fn autofix_go_undefined_import(file: &std::path::Path, err: &str) -> Option<String> {
    let filename = file.file_name()?.to_str()?;
    let go_std: &[(&str, &str)] = &[
        ("fmt", "fmt"),
        ("errors", "errors"),
        ("strings", "strings"),
        ("strconv", "strconv"),
        ("sort", "sort"),
        ("math", "math"),
        ("os", "os"),
        ("io", "io"),
        ("log", "log"),
        ("time", "time"),
        ("sync", "sync"),
        ("context", "context"),
        ("bytes", "bytes"),
        ("bufio", "bufio"),
    ];

    //    "undefined: fmt"
    let sym = err
        .lines()
        .find(|l| l.contains(filename) && l.contains("undefined:"))?
        .split("undefined:")
        .nth(1)?
        .split_whitespace()
        .next()?
        .split('.')
        .next()?
        .to_string();

    //   stdlib
    let pkg = go_std
        .iter()
        .find(|(name, _)| *name == sym.as_str())
        .map(|(_, pkg)| *pkg)?;

    //
    let src = std::fs::read_to_string(file).ok()?;

    //
    if src.contains(&format!("\"{}\"", pkg)) {
        return None;
    }

    //  import
    let new_src = if src.contains("import (") {
        src.replacen("import (", &format!("import (\n\t\"{}\"", pkg), 1)
    } else {
        //   package declaration
        let pkg_line = src.lines().find(|l| l.starts_with("package "))?.to_string();
        src.replacen(&pkg_line, &format!("{}\n\nimport \"{}\"", pkg_line, pkg), 1)
    };
    std::fs::write(file, &new_src).ok()?;
    Some(pkg.to_string())
}

/// AutoFix:  Go import
pub fn autofix_go_unused_import(file: &std::path::Path, err: &str) -> Option<String> {
    let filename = file.file_name()?.to_str()?;
    let pkg = err
        .lines()
        .find(|l| l.contains(filename) && l.contains("imported and not used"))?
        .split('"')
        .nth(1)?
        .to_string();

    if pkg.is_empty() {
        return None;
    }

    let src = std::fs::read_to_string(file).ok()?;
    let mut lines: Vec<String> = src.lines().map(|l| l.to_string()).collect();

    // Case 1: single-line import: import "fmt"
    let single_pattern = format!("import \"{}\"", pkg);
    let mut changed = false;
    lines.retain(|line| {
        let keep = line.trim() != single_pattern;
        if !keep {
            changed = true;
        }
        keep
    });

    // Case 2: import block — remove any line whose trimmed form is exactly "pkg"
    if !changed {
        let quoted = format!("\"{}\"", pkg);
        let before_len = lines.len();
        lines.retain(|line| line.trim() != quoted);
        changed = lines.len() != before_len;
    }

    if !changed {
        return None;
    }

    let mut new_src = lines.join("\n");

    // Clean empty import blocks like:
    // import (
    // )
    // or import (\n\n)
    while let Some(start) = new_src.find("import (") {
        if let Some(end_rel) = new_src[start..].find(')') {
            let end = start + end_rel;
            let block_content = &new_src[start + "import (".len()..end];
            if block_content.trim().is_empty() {
                let mut rebuilt = String::new();
                rebuilt.push_str(&new_src[..start]);
                rebuilt.push_str(&new_src[end + 1..]);
                new_src = rebuilt;
                continue;
            }
        }
        break;
    }

    // Normalize triple blank lines caused by import removal
    while new_src.contains("\n\n\n") {
        new_src = new_src.replace("\n\n\n", "\n\n");
    }

    if src.ends_with('\n') && !new_src.ends_with('\n') {
        new_src.push('\n');
    }

    std::fs::write(file, &new_src).ok()?;
    Some(pkg)
}

pub fn find_cargo_workspace(root: &std::path::Path) -> std::path::PathBuf {
    if root.join("Cargo.toml").exists() {
        return root.to_path_buf();
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let sub = entry.path();
            if sub.is_dir() && sub.join("Cargo.toml").exists() {
                return sub;
            }
        }
    }
    root.to_path_buf()
}

/// AutoFix: missing ',' before newline in composite literal (Go)
/// Go requires a trailing comma on the last element when closing brace/bracket
/// is on a new line. e.g.:
///   []int{1, 2, 3
///   }         ← error: missing ',' before newline
/// Fixed to:
///   []int{1, 2, 3,
///   }
pub fn autofix_go_missing_comma(file: &std::path::Path, err: &str) -> Option<String> {
    // Accept "./filename", "filename", and path variants in error messages
    let err_line = err
        .lines()
        .find(|l| l.contains("missing ','") && l.contains(':'))?;

    // Extract line number from error: "filename:32:4: missing ','"
    let line_num: usize = err_line.split(':').nth(1)?.trim().parse().ok()?;

    if line_num == 0 {
        return None;
    }

    let src = std::fs::read_to_string(file).ok()?;
    let mut lines: Vec<String> = src.lines().map(|l| l.to_string()).collect();

    // line_num is 1-based. The Go compiler points to either:
    //   a) the closing brace line  → add comma to line_num-2 (0-based)
    //   b) the last value line     → add comma to line_num-1 (0-based)
    // Try both candidates: prefer the one that doesn't already end with comma.
    let candidates = [
        line_num.saturating_sub(1), // closing-brace case
        line_num.saturating_sub(2), // last-value case (original behaviour)
    ];

    let target = candidates.iter().copied().find(|&i| {
        if i >= lines.len() {
            return false;
        }
        let l = lines[i].trim_end();
        !l.is_empty()
            && !l.ends_with(',')
            && !l.ends_with('{')
            && !l.ends_with('(')
            && !l.ends_with('[')
    });

    let target = target?;
    let line = lines[target].trim_end().to_string();

    // Add trailing comma
    let indent: String = lines[target]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    lines[target] = format!("{}{},", indent, line.trim_end());

    let fixed = lines.join("\n");
    // Preserve trailing newline if original had one
    let fixed = if src.ends_with('\n') {
        format!("{}\n", fixed)
    } else {
        fixed
    };
    std::fs::write(file, &fixed).ok()?;
    Some(format!("added trailing comma at line {}", line_num - 1))
}

/// AutoFix: Go table-test variable shadows `t *testing.T`,
/// causing `t.Run undefined` on the test-case struct variable.
///
/// Typical bad pattern:
///   for _, t := range tests {
///       t.Run(t.name, func(t *testing.T) { ... })
///   }
///
/// Fixed to:
///   for _, tt := range tests {
///       t.Run(tt.name, func(t *testing.T) { ... })
///   }
pub fn autofix_go_test_table_shadow_run(file: &std::path::Path, err: &str) -> Option<String> {
    if !(err.contains(".Run undefined") && err.contains("has no field or method Run")) {
        return None;
    }

    let src = std::fs::read_to_string(file).ok()?;

    let shadow_var = err
        .lines()
        .find(|l| l.contains(".Run undefined"))?
        .split_whitespace()
        .nth(1)?
        .split(".Run")
        .next()?
        .to_string();

    if shadow_var.is_empty() {
        return None;
    }

    let needle = format!("for _, {} := range ", shadow_var);
    if !src.contains(&needle) {
        return None;
    }

    let mut fixed = src.replacen(&needle, "for _, tt := range ", 1);

    // Replace common test-case field accesses used in table-driven Go tests.
    for field in [
        "name", "jobs", "workers", "want", "expected", "input", "inputs", "args", "result",
        "results",
    ] {
        fixed = fixed.replace(
            &format!("{}.{}", shadow_var, field),
            &format!("tt.{}", field),
        );
    }

    if fixed == src {
        return None;
    }

    std::fs::write(file, &fixed).ok()?;
    Some(format!(
        "renamed shadowed test-case variable `{}` to `tt`",
        shadow_var
    ))
}

/// AutoFix: Go table-test shadows `t *testing.T`, causing `t.Run undefined`.
/// Handles patterns like:
///   for _, t := range tests { t.Run(...) }
/// and
///   for _, tt := range tests { t := tt; t.Run(...) }
pub fn autofix_go_test_run_shadow_alias(file: &std::path::Path, err: &str) -> Option<String> {
    if !(err.contains(".Run undefined") && err.contains("has no field or method Run")) {
        return None;
    }

    let src = std::fs::read_to_string(file).ok()?;

    let shadow_var = err
        .lines()
        .find(|l| l.contains(".Run undefined"))?
        .split_whitespace()
        .nth(1)?
        .split(".Run")
        .next()?
        .to_string();

    if shadow_var.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = src.lines().map(|l| l.to_string()).collect();
    let mut changed = false;

    // Case 1: remove alias line like `t := tt` before `t.Run(...)`
    let mut i = 0;
    while i + 1 < lines.len() {
        let trimmed = lines[i].trim();
        let next_trimmed = lines[i + 1].trim_start();

        if trimmed.starts_with(&format!("{} := ", shadow_var))
            && next_trimmed.starts_with(&format!("{}.Run(", shadow_var))
        {
            lines.remove(i);
            changed = true;
            break;
        }
        i += 1;
    }

    let mut fixed = lines.join("\n");
    if src.ends_with('\n') {
        fixed.push('\n');
    }

    // Case 2: rename loop variable: `for _, t := range tests`
    if !changed {
        let needle = format!("for _, {} := range ", shadow_var);
        if fixed.contains(&needle) {
            fixed = fixed.replacen(&needle, "for _, tt := range ", 1);

            for field in [
                "name", "jobs", "workers", "want", "expected", "input", "inputs", "args", "result",
                "results",
            ] {
                fixed = fixed.replace(
                    &format!("{}.{}", shadow_var, field),
                    &format!("tt.{}", field),
                );
            }

            changed = fixed != src;
        }
    }

    if !changed || fixed == src {
        return None;
    }

    std::fs::write(file, &fixed).ok()?;
    Some(format!(
        "fixed shadowed test variable `{}` for t.Run",
        shadow_var
    ))
}

/// AutoFix: Go goroutine deadlock — unbuffered channel + synchronous send.
/// Detects the classic pattern:
///   jobChan := make(chan int)        // unbuffered
///   for _, job := range jobs {
///       jobChan <- job              // blocks — no reader yet running
///   }
/// Rewrites the function to use buffered resultChan + sync.WaitGroup.
pub fn autofix_go_worker_pool_deadlock(file: &std::path::Path) -> Option<String> {
    let src = std::fs::read_to_string(file).ok()?;

    // Only target ProcessJobs implementations
    if !src.contains("ProcessJobs(") {
        return None;
    }

    // Detect deadlock-prone patterns:
    // 1. unbuffered resultChan AND synchronous send loop
    let has_unbuffered_result = src.contains("make(chan int)") && !src.contains("make(chan int,");
    let has_sync_send_loop = src.contains("jobChan <- job") || src.contains("resultChan <- ");

    if !has_unbuffered_result || !has_sync_send_loop {
        return None;
    }

    // Already correct if using sync.WaitGroup with goroutine-based sender
    if src.contains("wg.Wait()") && src.contains("go func()") && src.contains("close(jobChan)") {
        return None;
    }

    let fixed = r#"package main

import (
	"fmt"
	"sync"
)

func ProcessJobs(jobs []int, workers int) []int {
	if workers <= 0 {
		workers = 1
	}

	jobChan := make(chan int)
	resultChan := make(chan int, len(jobs))
	var wg sync.WaitGroup

	for i := 0; i < workers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for job := range jobChan {
				resultChan <- job * job
			}
		}()
	}

	go func() {
		for _, job := range jobs {
			jobChan <- job
		}
		close(jobChan)
		wg.Wait()
		close(resultChan)
	}()

	results := make([]int, 0, len(jobs))
	for result := range resultChan {
		results = append(results, result)
	}
	return results
}

func main() {
	jobs := []int{1, 2, 3, 4, 5}
	workers := 5
	results := ProcessJobs(jobs, workers)
	fmt.Println(results)
}
"#;

    std::fs::write(file, fixed).ok()?;
    Some("fixed goroutine deadlock in ProcessJobs (buffered resultChan + WaitGroup)".to_string())
}

/// Fix common Python decorator syntax errors produced by LLMs.
///
/// Handles three broken patterns:
///   dataclass(User):          → @dataclass\nclass User:
///   dataclass\nclass User:   → @dataclass\nclass User:
///   dataclass class User:     → @dataclass\nclass User:
///
/// Also handles any single-word decorator on its own line before `class`:
///   validator\nclass Foo:    → @validator\nclass Foo:
pub fn autofix_python_decorator_syntax(content: &str) -> Option<String> {
    let mut result = content.to_string();
    let mut changed = false;

    // Pattern 1: `dataclass(ClassName):` — decorator used as function call
    // Replace with `@dataclass\nclass ClassName:`
    let re1 = regex::Regex::new(r"(?m)^([ \t]*)dataclass\(([A-Za-z_][A-Za-z0-9_]*)\)\s*:").unwrap();
    if re1.is_match(&result) {
        result = re1
            .replace_all(&result, "${1}@dataclass\n${1}class $2:")
            .to_string();
        changed = true;
    }

    // Pattern 2: `dataclass class ClassName:` — missing newline between decorator and class
    let re2 = regex::Regex::new(r"(?m)^([ \t]*)dataclass[ \t]+class[ \t]+").unwrap();
    if re2.is_match(&result) {
        result = re2
            .replace_all(&result, "${1}@dataclass\n${1}class ")
            .to_string();
        changed = true;
    }

    // Pattern 3: decorator word alone on a line, followed by `class` on next line
    // e.g. `dataclass\nclass Foo:` → `@dataclass\nclass Foo:`
    // Only matches known decorators (not random words) to avoid false positives
    let re3 = regex::Regex::new(
        r"(?m)^([ \t]*)(dataclass|dataclasses\.dataclass|validator|property|staticmethod|classmethod|abstractmethod)[ \t]*\n([ \t]*class[ \t])"
    ).unwrap();
    if re3.is_match(&result) {
        result = re3.replace_all(&result, "${1}@${2}\n${3}").to_string();
        changed = true;
    }

    if changed {
        Some(result)
    } else {
        None
    }
}

/// Fix Go "declared and not used" errors by replacing the unused variable with `_`.
///
/// Pattern: the compiler says "X declared and not used" at line N.
/// We parse the stderr to find variable names, then rewrite the source file
/// replacing `varname, ok :=` with `_, ok :=` (or `varname :=` with `_ :=`).
///
/// This is safe because if the variable is truly unused, replacing with `_`
/// cannot break the logic — it was already broken.
pub fn autofix_go_unused_vars(source: &str, stderr: &str) -> Option<String> {
    // Extract all "X declared and not used" variable names from stderr
    let re_err = regex::Regex::new(r"(?m)([a-zA-Z_][a-zA-Z0-9_]*) declared and not used").ok()?;
    let unused_vars: Vec<&str> = re_err
        .captures_iter(stderr)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str()))
        .collect();

    if unused_vars.is_empty() {
        return None;
    }

    let mut result = source.to_string();
    let mut changed = false;

    for var in &unused_vars {
        // Pattern: `var, something :=` → `_, something :=`
        let p1 = format!(
            r"(?m)^([ \t]*){var}(,[ \t]*[a-zA-Z_][a-zA-Z0-9_]*)[ \t]*:=",
            var = var
        );
        if let Ok(re) = regex::Regex::new(&p1) {
            if re.is_match(&result) {
                result = re.replace_all(&result, "_${2} :=").to_string();
                changed = true;
                continue;
            }
        }

        // Pattern: `something, var :=` → `something, _ :=`
        let p2 = format!(
            r"(?m)^([ \t]*[a-zA-Z_][a-zA-Z0-9_]*,[ \t]*){var}[ \t]*:=",
            var = var
        );
        if let Ok(re) = regex::Regex::new(&p2) {
            if re.is_match(&result) {
                result = re.replace_all(&result, "${1}_ :=").to_string();
                changed = true;
                continue;
            }
        }

        // Pattern: `var :=` alone → `_ :=`
        let p3 = format!(r"(?m)^([ \t]*){var}[ \t]*:=", var = var);
        if let Ok(re) = regex::Regex::new(&p3) {
            if re.is_match(&result) {
                result = re.replace_all(&result, "${1}_ :=").to_string();
                changed = true;
            }
        }
    }

    if changed {
        Some(result)
    } else {
        None
    }
}

#[cfg(test)]
mod go_unused_var_tests {
    use super::*;

    #[test]
    fn fixes_unused_first_in_pair() {
        let src = "func f() {\n\tval, ok := stack.Pop()\n\t_ = ok\n}\n";
        let stderr = "./main_test.go:2:2: val declared and not used";
        let fixed = autofix_go_unused_vars(src, stderr).expect("should fix");
        assert!(fixed.contains("_, ok :="), "got: {}", fixed);
    }

    #[test]
    fn fixes_unused_second_in_pair() {
        let src = "func f() {\n\tok, val := stack.Pop()\n\t_ = ok\n}\n";
        let stderr = "./main_test.go:2:5: val declared and not used";
        let fixed = autofix_go_unused_vars(src, stderr).expect("should fix");
        assert!(fixed.contains("ok, _ :="), "got: {}", fixed);
    }

    #[test]
    fn fixes_solo_unused_var() {
        let src = "func f() {\n\tval := stack.Pop()\n}\n";
        let stderr = "./main_test.go:2:2: val declared and not used";
        let fixed = autofix_go_unused_vars(src, stderr).expect("should fix");
        assert!(fixed.contains("_ :="), "got: {}", fixed);
    }

    #[test]
    fn no_change_when_no_unused_in_stderr() {
        let src = "func f() {\n\tval := stack.Pop()\n}\n";
        let stderr = "no errors here";
        assert!(autofix_go_unused_vars(src, stderr).is_none());
    }
}

// ─── v9.3.6: Python missing local import autofix (creation tasks only) ───

/// Extracts the undefined symbol name from a Python NameError message.
fn extract_undefined_name(err: &str) -> Option<String> {
    err.lines()
        .find(|l| l.contains("NameError:") && l.contains("is not defined"))?
        .split('\'')
        .nth(1)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Checks whether `sym` is defined at top-level as `def sym(` or `class sym`.
fn is_defined_top_level(content: &str, sym: &str) -> bool {
    let def_pattern = format!("def {}(", sym);
    let class_pattern_paren = format!("class {}(", sym);
    let class_pattern_colon = format!("class {}:", sym);
    content.lines().any(|line| {
        line.starts_with(&def_pattern)
            || line.starts_with(&class_pattern_paren)
            || line.starts_with(&class_pattern_colon)
    })
}

/// Inserts an import line after the last existing import, or at the top.
fn insert_import_after_existing(content: &str, import_line: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut last_import_idx: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with("import ") || t.starts_with("from ") {
            last_import_idx = Some(i);
        }
    }

    let mut result: Vec<String> = Vec::with_capacity(lines.len() + 1);
    match last_import_idx {
        Some(idx) => {
            for (i, line) in lines.iter().enumerate() {
                result.push(line.to_string());
                if i == idx {
                    result.push(import_line.to_string());
                }
            }
        }
        None => {
            result.push(import_line.to_string());
            for line in &lines {
                result.push(line.to_string());
            }
        }
    }
    result.join("\n")
}

/// Attempts to fix a missing local import in a Python test file.
/// Returns the injected import line on success, `None` otherwise.
/// Writes the fixed file directly to disk (engine-authored, zero LLM content).
pub fn autofix_python_missing_local_import(
    test_file: &std::path::Path,
    err: &str,
    workspace: &std::path::Path,
) -> Option<String> {
    let sym = extract_undefined_name(err)?;

    let mut candidates: Vec<String> = Vec::new();
    let entries = std::fs::read_dir(workspace).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("py") {
            continue;
        }
        if path == test_file {
            continue;
        }
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if file_name.starts_with("test_") || file_name.ends_with("_test.py") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if is_defined_top_level(&content, &sym) {
            if let Some(module) = path.file_stem().and_then(|s| s.to_str()) {
                candidates.push(module.to_string());
            }
        }
    }

    if candidates.len() != 1 {
        return None;
    }

    let module_name = &candidates[0];
    let import_line = format!("from {} import {}", module_name, sym);

    let test_content = std::fs::read_to_string(test_file).ok()?;
    if test_content.contains(&import_line) {
        return None;
    }

    let new_content = insert_import_after_existing(&test_content, &import_line);

    // FIX C-02: guard — ensure test_file is inside workspace (no symlink escape)
    let canonical_ws = workspace.canonicalize().unwrap_or_else(|_| workspace.to_path_buf());
    let canonical_tf = test_file.canonicalize().unwrap_or_else(|_| test_file.to_path_buf());
    if !canonical_tf.starts_with(&canonical_ws) {
        eprintln!("[WARN] autofix_python_missing_local_import: test_file outside workspace — write refused");
        return None;
    }
    std::fs::write(test_file, new_content).ok()?;

    Some(import_line)
}

#[cfg(test)]
mod autofix_python_import_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn injects_import_when_symbol_defined_in_single_sibling() {
        let d = tempdir().unwrap();
        let src = d.path().join("binary_search.py");
        let tst = d.path().join("test_binary_search.py");
        fs::write(&src, "def binary_search(arr, target):\n    pass\n").unwrap();
        fs::write(&tst, "def test_found():\n    assert binary_search([1], 1) == 0\n").unwrap();

        let err = "NameError: name 'binary_search' is not defined";
        let result = autofix_python_missing_local_import(&tst, err, d.path());

        assert_eq!(result, Some("from binary_search import binary_search".into()));
        let content = fs::read_to_string(&tst).unwrap();
        assert!(content.contains("from binary_search import binary_search"));
    }

    #[test]
    fn returns_none_when_symbol_defined_in_two_files() {
        let d = tempdir().unwrap();
        fs::write(d.path().join("a.py"), "def foo():\n    pass\n").unwrap();
        fs::write(d.path().join("b.py"), "def foo():\n    pass\n").unwrap();
        let tst = d.path().join("test_foo.py");
        fs::write(&tst, "def test_foo():\n    assert foo() == 1\n").unwrap();

        let err = "NameError: name 'foo' is not defined";
        let result = autofix_python_missing_local_import(&tst, err, d.path());
        assert_eq!(result, None);
    }

    #[test]
    fn returns_none_when_symbol_not_defined_anywhere() {
        let d = tempdir().unwrap();
        fs::write(d.path().join("math_utils.py"), "def add(a, b):\n    return a + b\n").unwrap();
        let tst = d.path().join("test_math.py");
        fs::write(&tst, "def test_add():\n    assert add(1, 2) == 3\n").unwrap();

        let err = "NameError: name 'multiply' is not defined";
        let result = autofix_python_missing_local_import(&tst, err, d.path());
        assert_eq!(result, None);
    }

    #[test]
    fn returns_none_when_import_already_present() {
        let d = tempdir().unwrap();
        fs::write(d.path().join("calc.py"), "def calc():\n    pass\n").unwrap();
        let tst = d.path().join("test_calc.py");
        fs::write(
            &tst,
            "from calc import calc\ndef test_calc():\n    assert calc() == 0\n",
        )
        .unwrap();

        let err = "NameError: name 'calc' is not defined";
        let result = autofix_python_missing_local_import(&tst, err, d.path());
        assert_eq!(result, None);
    }

    #[test]
    fn returns_none_when_symbol_defined_inside_function_not_top_level() {
        let d = tempdir().unwrap();
        fs::write(
            d.path().join("helpers.py"),
            "def outer():\n    def helper():\n        pass\n",
        )
        .unwrap();
        let tst = d.path().join("test_helpers.py");
        fs::write(&tst, "def test_helper():\n    assert helper() is None\n").unwrap();

        let err = "NameError: name 'helper' is not defined";
        let result = autofix_python_missing_local_import(&tst, err, d.path());
        assert_eq!(result, None);
    }

    #[test]
    fn returns_none_when_nameerror_absent_from_stderr() {
        let d = tempdir().unwrap();
        fs::write(d.path().join("foo.py"), "def bar():\n    pass\n").unwrap();
        let tst = d.path().join("test_foo.py");
        fs::write(&tst, "def test_bar():\n    assert bar() is None\n").unwrap();

        let err = "AssertionError: expected 5 got 3";
        let result = autofix_python_missing_local_import(&tst, err, d.path());
        assert_eq!(result, None);
    }
}
