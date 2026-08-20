use crate::executor::core::*;

#[derive(Debug, PartialEq)]
pub enum MutationResult {
    Strong,
    Weak(String, String), // (original_line, mutated_line)
    /// Mutation produced a compile/parse error (never actually ran).
    /// Must NOT be counted as killed and NOT as equivalent — the mutation is invalid noise.
    Uncompilable(String, String), // (original_line, mutated_line)
    Skipped(String),      // Reason
}

/// True when non-zero-exit stderr shows a compile/parse/type failure
/// rather than a real test assertion failure.
/// Uses crate::diagnostic when available, then falls back to language-agnostic markers.
pub fn stderr_is_compile_failure(stderr: &str) -> bool {
    // Reuse existing analyzer signal first.
    let report = crate::diagnostic::analyze(stderr);
    if report.hints.iter().any(|h| {
        matches!(
            h.category,
            "go/undefined"
                | "go/unused-var"
                | "go/unused-import"
                | "go/type-mismatch"
                | "rust/borrow"
                | "rust/move"
                | "rust/type"
                | "rust/E0422-not-pub"
                | "rust/E0422-visibility"
                | "rust/bootstrap"
                | "rust/integration-import"
                | "python/syntax"
                | "python/indent"
                | "python/dataclass-syntax"
        )
    }) {
        return true;
    }
    // Additional low-level markers (compiler/parser only, never test-assertion output).
    let l = stderr.to_ascii_lowercase();
    l.contains("syntaxerror")
        || l.contains("indentationerror")
        || l.contains("error[e0")               // Rust compile error codes
        || l.contains("could not compile")
        || l.contains("cannot find")
        || l.contains("expected identifier")
        || l.contains("expected `;`")
        || l.contains("unexpected token")
        || l.contains("undeclared name")
        || l.contains("undefined:")
        || l.contains("declared but not used")
        || l.contains("imported and not used")
        // TypeScript / tsc diagnostic codes
        || l.contains("ts1005")
        || l.contains("ts1109")
        || l.contains("ts1128")
        || l.contains("ts1136")
        || l.contains("ts2304")
        || l.contains("ts2339")
        || l.contains("ts2551")
        || l.contains("ts2552")
}

pub fn apply_all_mutations(code: &str) -> Vec<(String, String, String)> {
    let strategies: &[(&str, &str)] = &[
        //  Operators (existing)
        ("==", "!="),
        ("!=", "=="),
        (" > ", " < "),
        (" < ", " > "),
        (" >= ", " <= "),
        (" <= ", " >= "),
        (" + ", " - "),
        (" - ", " + "),
        //  Boolean returns
        ("return True", "return False"),
        ("return False", "return True"),
        ("return true", "return false"), // Go/Rust/TS
        ("return false", "return true"), // Go/Rust/TS
        //  Constants (covers helper.py: return 42)
        ("return 0\n", "return 1\n"),
        ("return 1\n", "return 0\n"),
        ("return 42", "return 0"),
        ("return -1", "return 0"),
        //  Function swaps (covers max_of_three.py, min/max confusion)
        ("max(", "min("),
        ("min(", "max("),
        //  Multiplication (covers double.py: x*2)
        (" * 2", " * 3"),
        (" * 3", " * 2"),
        (" * ", " / "),
        //  Python slice reversal (covers reverse_string.py)
        ("[::-1]", "[::1]"),
        //  List methods (covers stack.py pop/append)
        (".append(", ".insert(0, "),
        //  Go/Rust: without-space arithmetic (covers go add: a+b)
        ("a+b", "a-b"),
        ("a-b", "a+b"),
        ("a + b", "a - b"),
        ("a - b", "a + b"),
        //  Logical operators (Go/TS/Rust)
        (" && ", " || "),
        (" || ", " && "),
        //  String operations (covers greet.py: f'Hi {name}')
        ("f'Hi {", "f'Bye {"),
        ("f\"Hi {", "f\"Bye {"),
        //  Modulo (covers is_even: n%2==0)  handled by == already
        //  Recursion (covers factorial: n-1)
        ("n - 1)", "n + 1)"),
        ("n - 1,", "n + 1,"),
    ];

    //    (loop variables  )
    let skip_patterns = [
        "i += ", "i -= ", "j += ", "j -= ", "idx", "index", "count +=", "count -=", "test", "Test",
        "assert", "expect", //
        "#[", "//", // Rust attributes and comments
    ];

    let mut result = Vec::new();
    for (from, to) in strategies {
        let mut found_line = None;
        let mutated: String = code
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                let skip = trimmed.starts_with('#')
                    || trimmed.starts_with("//")
                    || trimmed.starts_with('*')
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with("import ")
                    || trimmed.starts_with("use ")
                    || skip_patterns.iter().any(|p| line.contains(p));
                if found_line.is_none() && !skip && line.contains(*from) {
                    let new_line = line.replacen(from, to, 1);
                    found_line = Some((line.trim().to_string(), new_line.trim().to_string()));
                    new_line
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Some((orig, mutd)) = found_line {
            result.push((mutated, orig, mutd));
        }
    }
    result
}

impl SafeExecutor {
    pub async fn mutation_check(&self, source_file: &str) -> MutationResult {
        // في replay mode: mutation check يشغّل tests حقيقية خارج trajectory
        // هذا يكسر determinism ويسبب flakiness — نتجاوزه في replay
        if self.replay_mode {
            return MutationResult::Skipped("replay mode — skipped for determinism".into());
        }

        let source_path = self.workspace.join(source_file);
        if !source_path.exists() {
            return MutationResult::Skipped("Path not found".into());
        }
        let ext = source_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let original = match std::fs::read_to_string(&source_path) {
            Ok(s) => s,
            Err(_) => return MutationResult::Skipped("Read failure".into()),
        };
        let mutations = apply_all_mutations(&original);
        if mutations.is_empty() {
            return MutationResult::Skipped("No mutable patterns found".into());
        }
        // test runner per language
        let test_cmd: Vec<String> = match ext {
            "py" => {
                let pytest = if self.workspace.join("venv/bin/pytest").exists() {
                    "venv/bin/pytest"
                } else {
                    "pytest"
                };
                vec![
                    pytest.into(),
                    "-x".into(),
                    "-q".into(),
                    "--tb=no".into(),
                    "-p".into(),
                    "no:cacheprovider".into(),
                ]
            }
            "go" => vec![
                "go".into(),
                "test".into(),
                "./...".into(),
                "-count=1".into(),
            ],
            "js" | "ts" => {
                let npx = if self.workspace.join("node_modules/.bin/jest").exists() {
                    "node_modules/.bin/jest"
                } else {
                    "npx"
                };
                vec![
                    npx.into(),
                    "--forceExit".into(),
                    "--silent".into(),
                    "--no-cache".into(),
                ]
            }
            "rs" => vec!["cargo".into(), "test".into(), "--quiet".into()],
            _ => return MutationResult::Skipped("Unsupported lang for mutation".into()),
        };
        let mut survived_orig = String::new();
        let mut survived_mutd = String::new();
        let mut any_caught = false;
        let mut any_missed = false;
        for (mutation, orig_line, mutd_line) in &mutations {
            if std::fs::write(&source_path, mutation).is_err() {
                let _ = std::fs::write(&source_path, &original);
                continue;
            }
            // v7.5.3: Safety Buffer  wait for OS file sync/cache invalidation
            std::thread::sleep(std::time::Duration::from_millis(50));
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                tokio::process::Command::new(&test_cmd[0])
                    .args(&test_cmd[1..])
                    .current_dir(&self.workspace)
                    .env("PYTHONPATH", &self.workspace)
                    .env("PYTHONDONTWRITEBYTECODE", "1")
                    .output(),
            )
            .await;
            let _ = std::fs::write(&source_path, &original);
            if let Ok(Ok(result)) = out {
                if result.status.success() {
                    if !any_missed {
                        survived_orig = orig_line.clone();
                        survived_mutd = mutd_line.clone();
                    }
                    any_missed = true;
                } else {
                    // Distinguish real test-kill from compile/parse failure.
                    let combined = format!(
                        "{}\n{}",
                        String::from_utf8_lossy(&result.stderr),
                        String::from_utf8_lossy(&result.stdout)
                    );
                    if stderr_is_compile_failure(&combined) {
                        // Skip this mutation entirely — invalid noise, not a kill.
                        let _ = std::fs::write(&source_path, &original);
                        return MutationResult::Uncompilable(orig_line.clone(), mutd_line.clone());
                    }
                    any_caught = true;
                }
            }
            if any_missed {
                break;
            }
        }
        let _ = std::fs::write(&source_path, &original);
        if any_missed {
            MutationResult::Weak(survived_orig, survived_mutd)
        } else if any_caught {
            MutationResult::Strong
        } else {
            MutationResult::Skipped("No survivors".into())
        }
    }
}

#[cfg(test)]
mod stderr_classifier_tests {
    use super::stderr_is_compile_failure;

    #[test]
    fn ts_syntax_error_is_compile_failure() {
        let s = "src/x.ts:3:12 - error TS1005: ',' expected.";
        assert!(stderr_is_compile_failure(s));
    }

    #[test]
    fn ts_unexpected_token_is_compile_failure() {
        let s = "SyntaxError: Unexpected token '!'";
        assert!(stderr_is_compile_failure(s));
    }

    #[test]
    fn python_syntax_error_is_compile_failure() {
        let s = "  File \"x.py\", line 3\n    if a !!= 2:\n         ^\nSyntaxError: invalid syntax";
        assert!(stderr_is_compile_failure(s));
    }

    #[test]
    fn rust_compile_error_is_compile_failure() {
        let s = "error[E0308]: mismatched types\n  --> src/lib.rs:1:1";
        assert!(stderr_is_compile_failure(s));
    }

    #[test]
    fn go_undefined_is_compile_failure() {
        let s = "./main.go:5:6: undefined: FooBar";
        assert!(stderr_is_compile_failure(s));
    }

    #[test]
    fn real_test_assertion_is_not_compile_failure() {
        let s = "FAIL src/x.test.ts\n  expected 1 to equal 2\n  at Object.<anonymous>";
        assert!(!stderr_is_compile_failure(s));
    }

    #[test]
    fn pytest_assertion_is_not_compile_failure() {
        let s = "test_x.py::test_add FAILED\n    assert add(1,2) == 4\nAssertionError";
        assert!(!stderr_is_compile_failure(s));
    }
}
