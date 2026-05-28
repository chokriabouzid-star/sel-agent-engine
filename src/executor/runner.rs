use crate::executor::autofix::*;
use crate::executor::core::*;
use crate::executor::parsers::*;
use crate::types::ExecResult;
use anyhow::{anyhow, Result};
use tokio::process::Command as TCmd;

impl SafeExecutor {
    //  RunTests 

    pub async fn run_tests(&self, target: &str) -> Result<ExecResult> {
        let (prog, args) = self.oracle.resolve_test_command(target);
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
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new("cargo")
                    .args(["test", "--", "--nocapture"])
                    .current_dir(&rust_ws)
                    .output(),
            )
            .await
            .map_err(|_| anyhow!("cargo test timeout"))??;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{}\n{}", stdout, stderr);

            // v1.4 Digest AutoFix logic (kept for robustness)
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
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success {
                    String::new()
                } else {
                    let s = combined.len().saturating_sub(2000);
                    combined[s..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
                autofix_triggered: false,
            });
        }

        // --- GO ---
        if prog == "go" || prog.ends_with("/go") {
            let mut autofix_active = false;
            // v8.0: go mod init is a purely LOCAL operation (no network)  allowed in replay mode
            if !self.workspace.join("go.mod").exists() {
                // v8.4.2: use workspace dir name, not hardcoded "sel_tmp"
                let mod_name = self.workspace
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("main")
                    .to_string();
                let mod_name = if mod_name.starts_with("sel-smoke")
                    || mod_name.starts_with("sel_tmp")
                    || mod_name.starts_with("tmp")
                {
                    "main".to_string()
                } else {
                    mod_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
                };
                println!("   ⚡ AutoFix: go.mod missing → initializing module '{}'", mod_name);
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

            // v8.0: go mod tidy is LOCAL  allowed in replay mode
            if self.workspace.join("go.mod").exists() {
                let _ = TCmd::new("go")
                    .args(["mod", "tidy"])
                    .current_dir(&self.workspace)
                    .output()
                    .await;
            }

            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new("go")
                    .args(&args)
                    .current_dir(&self.workspace)
                    .output(),
            )
            .await
            .map_err(|_| anyhow!("go test timeout"))??;

            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let exit_ok = out.status.success();
            let (passed, failed) = parse_go_tests(&combined);

            let no_test_files = combined.contains("[no test files]");
            let success = exit_ok && (passed > 0 || no_test_files);

            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success {
                    String::new()
                } else {
                    let s = combined.len().saturating_sub(2000);
                    combined[s..].to_string()
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
            let mut out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new(&prog)
                    .args(&args)
                    .current_dir(&self.workspace)
                    .output(),
            )
            .await
            .map_err(|_| anyhow!("Node.js test timeout"))??;
            let mut combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );

            // AutoFix: Missing Node Module
            // v8.4.2: Allow in replay mode — npm install is local
            if combined.contains("Cannot find module '") {
                if let Some(start) = combined.find("Cannot find module '") {
                    let rest = &combined[start + "Cannot find module '".len()..];
                    if let Some(end) = rest.find('\'') {
                        let module = &rest[..end];
                        if !module.starts_with('.') && !module.starts_with('/') {
                            println!("   ⚡ QuickFix: npm install {}", module);
                            autofix_active = true;
                            let _ = TCmd::new("npm")
                                .args(["install", module])
                                .current_dir(&self.workspace)
                                .output()
                                .await;
                            
                            // Re-run tests after install
                            out = tokio::time::timeout(
                                std::time::Duration::from_secs(self.timeout_secs),
                                TCmd::new(&prog)
                                    .args(&args)
                                    .current_dir(&self.workspace)
                                    .output(),
                            )
                            .await
                            .map_err(|_| anyhow!("Node.js test timeout after AutoFix"))??;
                            
                            combined = format!(
                                "{}\n{}",
                                String::from_utf8_lossy(&out.stdout),
                                String::from_utf8_lossy(&out.stderr)
                            );
                        }
                    }
                }
            }
            let exit_ok = out.status.success();
            let passed = combined
                .lines()
                .filter(|l| l.contains("") || l.contains("") || l.contains("passed"))
                .count();
            let failed = combined
                .lines()
                .filter(|l| {
                    l.contains("") || l.contains("") || l.contains("failed") || l.contains("FAIL")
                })
                .count();
            let success = exit_ok && passed > 0;
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success {
                    String::new()
                } else {
                    let s = combined.len().saturating_sub(2000);
                    combined[s..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
                autofix_triggered: autofix_active,
            });
        }

        // --- PYTHON ---
        if prog.contains("pytest") || target.contains("pytest") {
            // v7.5.7: Proactive AutoFix for Python venv + pytest
            let mut final_prog = prog.clone();
            let mut autofix_active = false;
            if !self.replay_mode && !self.workspace.join("venv").exists() {
                println!("   ⚡ AutoFix: creating venv and installing pytest...");
                autofix_active = true;
                let _ = TCmd::new("python3")
                    .args(["-m", "venv", "venv"])
                    .current_dir(&self.workspace)
                    .output()
                    .await;

                // v7.5.8: Use ABSOLUTE path for venv-specific pip to ensure success on first try
                let pip_bin = self.workspace.join("venv/bin/pip");
                let _ = TCmd::new(pip_bin)
                    .args(["install", "pytest", "--quiet"])
                    .current_dir(&self.workspace)
                    .output()
                    .await;

                // v7.5.1: Update prog to use the new venv
                if self.workspace.join("venv/bin/pytest").exists() {
                    final_prog = "venv/bin/pytest".to_string();
                }
            } else if prog == "pytest" && self.workspace.join("venv/bin/pytest").exists() {
                // v7.5.1: Fallback  if venv exists but model suggested 'pytest'
                final_prog = "venv/bin/pytest".to_string();
            }

            let py3 = self.workspace.join("venv/bin/python3");
            let py = self.workspace.join("venv/bin/python");

            let mut cmd = if final_prog == "venv/bin/pytest" && py3.exists() {
                let mut c = TCmd::new(&py3);
                c.arg("-m").arg("pytest");
                c
            } else if final_prog == "venv/bin/pytest" && py.exists() {
                let mut c = TCmd::new(&py);
                c.arg("-m").arg("pytest");
                c
            } else {
                TCmd::new(&final_prog)
            };

            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                cmd.args(&args)
                    .current_dir(&self.workspace)
                    .env("PYTHONPATH", &self.workspace)
                    .env("PYTHONDONTWRITEBYTECODE", "1")
                    .output(),
            )
            .await
            .map_err(|_| anyhow!("pytest timeout"))??;
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
                    let s = combined.len().saturating_sub(2000);
                    combined[s..].to_string()
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

