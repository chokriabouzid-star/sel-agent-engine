use crate::executor::autofix::*;
use crate::executor::core::*;
use crate::executor::parsers::*;
use crate::types::ExecResult;
use anyhow::Result;
use tokio::process::Command as TCmd;

fn capture_stderr(combined: &str, max_chars: usize) -> String {
    if combined.len() <= max_chars {
        return combined.to_string();
    }
    let head_size = max_chars * 2 / 3;
    let tail_size = max_chars - head_size;
    let head: String = combined.chars().take(head_size).collect();
    let tail_start = combined
        .char_indices()
        .rev()
        .nth(tail_size.saturating_sub(1))
        .map(|(i, _)| i)
        .unwrap_or(combined.len().saturating_sub(tail_size));
    format!(
        "{}
...[truncated]...
{}",
        head,
        &combined[tail_start..]
    )
}

impl SafeExecutor {
    pub async fn run_tests(&self, target: &str) -> Result<ExecResult> {
        let (prog, mut args) = self.oracle.resolve_test_command(target);
        let start = std::time::Instant::now();
        let p_type = self.oracle.current_type();

        println!(
            "   🚀 [Oracle:{:?}] Running: {} {}",
            p_type,
            prog,
            args.join(" ")
        );

        // --- RUST ---
        if prog == "cargo" || prog.ends_with("/cargo") {
            let rust_ws = find_cargo_workspace(&self.workspace);
            let mut cmd = TCmd::new("cargo");
            cmd.args(["test", "--", "--nocapture"])
                .current_dir(&rust_ws);

            if self.replay_mode {
                cmd.env("CARGO_NET_OFFLINE", "true")
                    .env("CARGO_NET_RETRY", "0");
                eprintln!("[TRACE] Rust replay: forcing cargo offline mode");
            }

            let out = match tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                cmd.output(),
            )
            .await
            {
                Err(_) => {
                    return Ok(ExecResult {
                        success: false,
                        exit_code: -1,
                        stdout: String::new(),
                        stderr: "cargo test timeout: test suite exceeded time limit.".into(),
                        duration_ms: start.elapsed().as_millis() as u64,
                        autofix_triggered: false,
                    })
                }
                Ok(r) => r?,
            };

            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{}\n{}", stdout, stderr);

            if combined.contains("trait `Digest` which provides") {
                println!("   ⚡ AutoFix: adding sha2::Digest import");
                for dir in [rust_ws.as_path(), rust_ws.join("src").as_path()] {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                                if let Ok(content) = std::fs::read_to_string(&p) {
                                    if content.contains("sha2::Sha256")
                                        && !content.contains("use sha2::Digest")
                                    {
                                        let _ = std::fs::write(
                                            &p,
                                            format!("use sha2::Digest;\n{}", content),
                                        );
                                        println!(
                                            "    Fixed {:?}",
                                            p.file_name().unwrap_or_default()
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let exit_ok = out.status.success();
            let (passed, failed) = parse_rust_tests(&combined);
            let success = exit_ok && passed > 0;

            // v9.3.3: 0 tests با exit 0 → treat as MissingTests failure
            let zero_tests = exit_ok
                && passed == 0
                && failed == 0
                && !combined.contains("error[E")
                && !combined.contains("error:");
            let stderr_out = if zero_tests {
                format!(
                    "running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored
{}",
                    capture_stderr(&combined, 1000)
                )
            } else if success {
                String::new()
            } else {
                capture_stderr(&combined, 3000)
            };

            return Ok(ExecResult {
                success: success && !zero_tests,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: stderr_out,
                duration_ms: start.elapsed().as_millis() as u64,
                autofix_triggered: false,
            });
        }

        // --- GO ---
        if prog == "go" || prog.ends_with("/go") {
            let mut autofix_active = false;

            if !self.workspace.join("go.mod").exists() {
                let mod_name = self
                    .workspace
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("main")
                    .to_string();

                let mod_name = if mod_name.starts_with("sel-smoke")
                    || mod_name.starts_with("sel_tmp")
                    || mod_name.starts_with("tmp")
                {
                    "sel_tmp".to_string()
                } else {
                    mod_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
                };

                println!(
                    "   ⚡ AutoFix: go.mod missing → initializing module '{}'",
                    mod_name
                );
                autofix_active = true;

                let init_out = TCmd::new("go")
                    .args(["mod", "init", &mod_name])
                    .current_dir(&self.workspace)
                    .output()
                    .await;

                match init_out {
                    Ok(o) if o.status.success() => println!("   ✅ go mod init succeeded"),
                    Ok(o) => eprintln!(
                        "    go mod init failed: {}",
                        String::from_utf8_lossy(&o.stderr)
                    ),
                    Err(e) => eprintln!("    go mod init error: {}", e),
                }
            }

            if self.workspace.join("go.mod").exists() {
                let _ = TCmd::new("go")
                    .args(["mod", "tidy"])
                    .current_dir(&self.workspace)
                    .output()
                    .await;
            }

            if self
                .force_go_race
                .load(std::sync::atomic::Ordering::Relaxed)
                && !args.iter().any(|a| a == "-race")
            {
                args.insert(1, "-race".to_string());
                eprintln!("[TRACE] P0: injected -race -> go {}", args.join(" "));
            }
            let out = match tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new("go")
                    .args(&args)
                    .current_dir(&self.workspace)
                    .output(),
            )
            .await
            {
                Err(_) => return Ok(ExecResult {
                    success: false, exit_code: -1,
                    stdout: String::new(),
                    stderr: "go test timeout: test suite exceeded time limit. Likely caused by a deadlock, infinite loop, or time.Sleep in production code.".into(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    autofix_triggered: autofix_active,
                }),
                Ok(r) => r?,
            };

            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );

            let exit_ok = out.status.success();
            let (passed, failed) = parse_go_tests(&combined);
            let no_test_files = combined.contains("[no test files]");
            let no_tests_to_run = combined.contains("[no tests to run]");
            let success =
                exit_ok && failed == 0 && passed > 0 && !no_test_files && !no_tests_to_run;

            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success {
                    String::new()
                } else {
                    capture_stderr(&combined, 3000)
                },
                duration_ms: start.elapsed().as_millis() as u64,
                autofix_triggered: autofix_active,
            });
        }

        // --- NODE / TS ---
        if prog == "npm"
            || prog == "npx"
            || prog == "node"
            || prog.ends_with("/npm")
            || prog.ends_with("/npx")
            || prog.ends_with("/node")
        {
            let mut autofix_active = false;

            let mut out = match tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new(&prog)
                    .args(&args)
                    .current_dir(&self.workspace)
                    .output(),
            )
            .await
            {
                Err(_) => {
                    return Ok(ExecResult {
                        success: false,
                        exit_code: -1,
                        stdout: String::new(),
                        stderr: "Node.js test timeout: test suite exceeded time limit.".into(),
                        duration_ms: start.elapsed().as_millis() as u64,
                        autofix_triggered: false,
                    })
                }
                Ok(r) => r?,
            };

            let mut combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );

            if combined.contains("Cannot find module '") {
                if let Some(start_pos) = combined.find("Cannot find module '") {
                    let rest = &combined[start_pos + "Cannot find module '".len()..];
                    if let Some(end) = rest.find('\'') {
                        let module = &rest[..end];
                        if !module.starts_with('.') && !module.starts_with('/') {
                            if crate::executor::node_builtins::is_node_builtin(module) {
                                eprintln!(
                                    "   ⚠️  QuickFix skipped: '{}' is a Node.js built-in — use `import {{ ... }} from '{}'` instead of npm install",
                                    module, module
                                );
                            } else {
                                println!("   ⚡ QuickFix: npm install {}", module);
                                autofix_active = true;

                                let _ = TCmd::new("npm")
                                    // FIX H-04: --ignore-scripts prevents lifecycle script execution
                                    .args([
                                        "install",
                                        "--ignore-scripts",
                                        "--no-audit",
                                        "--no-fund",
                                        module,
                                    ])
                                    .current_dir(&self.workspace)
                                    .output()
                                    .await;

                                out = match tokio::time::timeout(
                                    std::time::Duration::from_secs(self.timeout_secs),
                                    TCmd::new(&prog)
                                        .args(&args)
                                        .current_dir(&self.workspace)
                                        .output(),
                                )
                                .await
                                {
                                    Err(_) => {
                                        return Ok(ExecResult {
                                            success: false,
                                            exit_code: -1,
                                            stdout: String::new(),
                                            stderr: "Node.js test timeout after AutoFix.".into(),
                                            duration_ms: start.elapsed().as_millis() as u64,
                                            autofix_triggered: true,
                                        })
                                    }
                                    Ok(r) => r?,
                                };

                                combined = format!(
                                    "{}\n{}",
                                    String::from_utf8_lossy(&out.stdout),
                                    String::from_utf8_lossy(&out.stderr)
                                );
                            }
                        }
                    }
                }
            }

            let exit_ok = out.status.success();
            let (passed, failed) = parse_jest(&combined);
            let success = exit_ok && failed == 0 && passed > 0;

            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success {
                    String::new()
                } else {
                    capture_stderr(&combined, 3000)
                },
                duration_ms: start.elapsed().as_millis() as u64,
                autofix_triggered: autofix_active,
            });
        }
        // --- PYTHON ---
        if prog.contains("pytest") || target.contains("pytest") {
            let mut final_prog = prog.clone();
            let mut autofix_active = false;

            let wants_workspace_venv = target.contains("venv/bin/pytest")
                || prog == "venv/bin/pytest"
                || prog.ends_with("/venv/bin/pytest");

            // Replay must honor the recorded environment.
            // If a recorded trajectory expects workspace venv, try restoring it
            // from scaffold cache instead of silently degrading to system pytest.
            if self.replay_mode
                && wants_workspace_venv
                && !self.workspace.join("venv/bin/pytest").exists()
            {
                let cached_venv = dirs::cache_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("~/.cache"))
                    .join("sel-agent/scaffold/python/venv");
                if cached_venv.exists() && cached_venv.join("bin/pytest").exists() {
                    let target_venv = self.workspace.join("venv");
                    if !target_venv.exists() {
                        match std::os::unix::fs::symlink(&cached_venv, &target_venv) {
                            Ok(_) => println!("   ⚡ Replay Env: restored cached Python venv"),
                            Err(e) => eprintln!("   [TRACE] replay venv symlink failed: {}", e),
                        }
                    }
                }
            }

            if !self.replay_mode && !self.workspace.join("venv").exists() {
                println!("   ⚡ AutoFix: creating venv and installing pytest...");
                autofix_active = true;

                let _ = TCmd::new("python3")
                    .args(["-m", "venv", "venv"])
                    .current_dir(&self.workspace)
                    .output()
                    .await;

                let pip_bin = self.workspace.join("venv/bin/pip");
                let _ = TCmd::new(pip_bin)
                    .args(["install", "pytest", "--quiet"])
                    .current_dir(&self.workspace)
                    .output()
                    .await;
            }

            if self.workspace.join("venv/bin/pytest").exists() {
                final_prog = "venv/bin/pytest".to_string();
            }

            let py3 = self.workspace.join("venv/bin/python3");
            let py = self.workspace.join("venv/bin/python");

            let mut cmd = if py3.exists() {
                let mut c = TCmd::new(&py3);
                c.arg("-m").arg("pytest");
                c
            } else if py.exists() {
                let mut c = TCmd::new(&py);
                c.arg("-m").arg("pytest");
                c
            } else if self.replay_mode && wants_workspace_venv {
                return Ok(ExecResult::fail(
                    "REPLAY_ENV_MISMATCH: recorded target requires venv/bin/pytest but no workspace/cached venv is available".to_string()
                ));
            } else if final_prog == "pytest" || final_prog == "venv/bin/pytest" {
                let mut c = TCmd::new("python3");
                c.arg("-m").arg("pytest");
                c
            } else {
                TCmd::new(&final_prog)
            };

            let out = match tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                cmd.args(&args)
                    .current_dir(&self.workspace)
                    .env("PYTHONPATH", &self.workspace)
                    .env("PYTHONDONTWRITEBYTECODE", "1")
                    .output(),
            )
            .await
            {
                Err(_) => {
                    return Ok(ExecResult {
                        success: false,
                        exit_code: -1,
                        stdout: String::new(),
                        stderr: "pytest timeout: test suite exceeded time limit.".into(),
                        duration_ms: start.elapsed().as_millis() as u64,
                        autofix_triggered: autofix_active,
                    })
                }
                Ok(Ok(out)) => out,
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(ExecResult::fail(format!(
                        "pytest runner not found in workspace or system (resolved target: '{}')",
                        final_prog
                    )));
                }
                Ok(Err(e)) => return Err(e.into()),
            };

            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );

            let (passed, failed) = parse_pytest(&combined);
            let has_error =
                combined.contains("ERROR collecting") || combined.contains("no tests ran");
            let success = passed > 0 && failed == 0 && !has_error;

            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success {
                    String::new()
                } else {
                    capture_stderr(&combined, 3000)
                },
                duration_ms: start.elapsed().as_millis() as u64,
                autofix_triggered: autofix_active,
            });
        }

        Ok(ExecResult {
            success: false,
            exit_code: 1,
            stdout: String::new(),
            stderr: format!("No test handler for program: {}", prog),
            duration_ms: start.elapsed().as_millis() as u64,
            autofix_triggered: false,
        })
    }
}

#[cfg(test)]
mod p0_race_timeout_tests {
    use crate::workspace_oracle::goal_requires_go_race;

    #[test]
    fn p0_goal_race_pos() {
        assert!(goal_requires_go_race("go test -race ./... must pass"));
        assert!(goal_requires_go_race("ensure -race ./... passes"));
        assert!(goal_requires_go_race("race detector must find nothing"));
        assert!(goal_requires_go_race(
            "1000 concurrent requests — no race conditions — must pass",
        ));
    }

    #[test]
    fn p0_goal_race_neg() {
        assert!(!goal_requires_go_race("go test ./..."));
        assert!(!goal_requires_go_race("cargo test --nocapture"));
        assert!(!goal_requires_go_race(""));
    }

    #[test]
    fn p0_timeout_classified_as_build_error() {
        use crate::failure::FailureKind;
        assert_eq!(
            FailureKind::classify("go test timeout: exceeded."),
            FailureKind::BuildError
        );
        assert_eq!(
            FailureKind::classify("cargo test timeout: exceeded."),
            FailureKind::BuildError
        );
        assert_eq!(
            FailureKind::classify("pytest timeout: exceeded."),
            FailureKind::BuildError
        );
        assert_eq!(
            FailureKind::classify("Node.js test timeout: exceeded."),
            FailureKind::BuildError
        );
    }

    #[test]
    fn p0_timeout_hint_not_generic() {
        use crate::diagnostic::analyze;
        let r = analyze("go test timeout: test suite exceeded time limit.");
        assert!(r.hints.iter().any(|h| h.category == "test/runner-timeout"));
        assert!(!r.hints.iter().any(|h| h.category == "generic"));
    }
}

#[cfg(test)]
mod p0_behavioral_tests {
    //! Real-toolchain regression coverage for P0:
    //! (a) force_go_race flips a racy-but-passing Go suite into a detector failure;
    //! (b) a runner timeout comes back as Ok(ExecResult::fail) — repairable — not Err.
    use crate::executor::SafeExecutor;
    use std::path::PathBuf;

    fn tool_available(tool: &str, arg: &str) -> bool {
        std::process::Command::new(tool)
            .arg(arg)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn fresh_ws(tag: &str) -> PathBuf {
        let ws = std::env::temp_dir().join(format!("sel_p0_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&ws);
        std::fs::create_dir_all(&ws).expect("workspace");
        ws
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(f)
    }

    // Canonical unsynchronised counter: passes plain `go test`, fails `go test -race`.
    const RACY_GO: &str = "package racy\n\nimport \"sync\"\n\nvar Counter int\n\nfunc Bump(n int) {\n\tvar wg sync.WaitGroup\n\tfor i := 0; i < n; i++ {\n\t\twg.Add(1)\n\t\tgo func() {\n\t\t\tdefer wg.Done()\n\t\t\tCounter++\n\t\t}()\n\t}\n\twg.Wait()\n}\n";
    const RACY_TEST_GO: &str =
        "package racy\n\nimport \"testing\"\n\nfunc TestBump(t *testing.T) {\n\tBump(50)\n}\n";

    #[test]
    fn p0_force_go_race_turns_racy_success_into_failure() {
        if !tool_available("go", "version") {
            eprintln!("skip: go toolchain not available");
            return;
        }
        let ws = fresh_ws("race");
        std::fs::write(ws.join("go.mod"), "module racy\n\ngo 1.21\n").expect("go.mod");
        std::fs::write(ws.join("racy.go"), RACY_GO).expect("racy.go");
        std::fs::write(ws.join("racy_test.go"), RACY_TEST_GO).expect("racy_test.go");

        let exec = SafeExecutor::new(ws, 180);

        let plain = block_on(exec.run_tests("go")).expect("run_tests must return Ok");
        assert!(
            plain.success,
            "plain go test must pass the racy suite: {}",
            plain.stderr
        );

        exec.set_force_go_race(true);
        let raced = block_on(exec.run_tests("go")).expect("run_tests must return Ok");
        assert!(!raced.success, "forced -race must fail the racy suite");
        assert!(
            raced.stderr.contains("DATA RACE"),
            "stderr must carry race detector output:\n{}",
            raced.stderr
        );
    }

    #[test]
    fn p0_runner_timeout_is_repairable_failure_not_err() {
        if !tool_available("node", "--version") {
            eprintln!("skip: node not available");
            return;
        }
        let ws = fresh_ws("timeout");
        // Unknown project type -> smart split -> ("node", ["sleep.js"]) -> Node branch.
        std::fs::write(ws.join("sleep.js"), "setTimeout(() => {}, 3000);\n").expect("sleep.js");

        let exec = SafeExecutor::new(ws, 1);
        let r = block_on(exec.run_tests("node sleep.js"))
            .expect("timeout must be Ok(ExecResult::fail), never Err");
        assert!(!r.success);
        assert_eq!(r.exit_code, -1);
        assert!(
            r.stderr.contains("Node.js test timeout"),
            "stderr: {}",
            r.stderr
        );
        // state_handlers maps Ok(!success) -> AgentState::Repairing; Err -> fatal.
    }
}
