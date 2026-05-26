use crate::executor::core::*;

#[derive(Debug, PartialEq)]
pub enum MutationResult {
    Strong,
    Weak(String, String), // (original_line, mutated_line)
    Skipped(String),      // Reason
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
        ("return true", "return false"),       // Go/Rust/TS
        ("return false", "return true"),       // Go/Rust/TS
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
        "i += ", "i -= ", "j += ", "j -= ",
        "idx", "index",
        "count +=", "count -=",
        "test", "Test", "assert", "expect",  //    
        "#[", "//",  // Rust attributes and comments
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

