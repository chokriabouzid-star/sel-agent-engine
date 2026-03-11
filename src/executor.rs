// src/executor.rs — v0.4: تنفيذ آمن

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::process::Command as TCmd;
use crate::types::{ExecResult, SafetyError};
use crate::protocol::Cmd;

const ALLOWED: &[&str] = &[
    "python3", "python",
    "venv/bin/python3", "venv/bin/python",
    "venv/bin/pip3",    "venv/bin/pip",
    "venv/bin/uvicorn", "venv/bin/gunicorn",
    "venv/bin/pytest",  "pytest",
    "node", "npm", "npx", "node_modules/.bin/jest",
    "cargo", "rustc", "git",
    "go",
    "mkdir", "touch", "ls", "cat", "cp", "mv",
    "echo", "find", "grep", "curl", "chmod",
    "node", "npm",
];

const BLOCKED: &[&str] = &[
    "sudo", "rm -rf", "mkfs", "dd if=",
    "| sh", "| bash", "curl | bash",
    "> /dev/", "/etc/", "/sys/", "/proc/",
];

pub struct SafeExecutor {
    pub workspace: PathBuf,
    timeout_secs: u64,
}

impl SafeExecutor {
    pub fn new(workspace: PathBuf, timeout_secs: u64) -> Self {
        Self { workspace, timeout_secs }
    }

    pub async fn run(&self, cmd: &Cmd) -> Result<ExecResult> {
        match cmd {
            Cmd::Run       { command }         => self.shell(command).await,
            Cmd::WriteFile { path, content }   => self.write_file(path, content),
            Cmd::AppendFile{ path, content }   => self.append_file(path, content),
            Cmd::ReadFile  { path }            => self.read_file(path),
            Cmd::Mkdir     { path }            => self.mkdir(path),
            Cmd::RunTests  { target }          => self.run_tests(target).await,
            Cmd::Done      { .. }              => Ok(ExecResult::ok("done")),
        }
    }

    // ─── Shell ─────────────────────────────────────

    async fn shell(&self, command: &str) -> Result<ExecResult> {
        self.safety_check(command)?;

        let parts: Vec<&str> = command.split_whitespace().collect();
        let prog = parts.first().ok_or_else(|| anyhow!("Empty command"))?;

        // رفض pip install بدون package name
        if (prog.contains("pip3") || prog.contains("pip")) {
            let is_install = parts.iter().any(|p| *p == "install");
            let has_package = parts.len() > 2 && parts.iter().skip(2).any(|p| !p.starts_with('-'));
            if is_install && !has_package {
                return Ok(ExecResult::fail(
                    "pip install needs package name: e.g. venv/bin/pip3 install pytest".to_string()
                ));
            }
        }

        if !ALLOWED.iter().any(|a| *a == *prog) {
            return Ok(ExecResult::fail(format!(
                "'{}' is not in the allowed programs list", prog
            )));
        }

        let services = ["venv/bin/uvicorn", "uvicorn", "venv/bin/gunicorn"];
        if services.contains(prog) {
            return self.service(prog, &parts[1..]).await;
        }

        let start = Instant::now();
        let out = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            TCmd::new(prog).args(&parts[1..]).current_dir(&self.workspace).output(),
        ).await
        .map_err(|_| anyhow!("Timeout after {}s: {}", self.timeout_secs, command))??;

        Ok(ExecResult {
            success:     out.status.success(),
            exit_code:   out.status.code().unwrap_or(-1),
            stdout:      String::from_utf8_lossy(&out.stdout).into(),
            stderr:      String::from_utf8_lossy(&out.stderr).into(),
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn service(&self, prog: &str, args: &[&str]) -> Result<ExecResult> {
        println!("   🌐 Service: {}", prog);
        TCmd::new(prog).args(args).current_dir(&self.workspace).spawn()?;
        tokio::time::sleep(Duration::from_millis(800)).await;
        Ok(ExecResult::ok("Service started"))
    }

    // ─── File Operations ───────────────────────────

    fn write_file(&self, path: &str, content: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
        // Rust brace balance check
        let content = if path.ends_with(".rs") {
            let open  = content.chars().filter(|&c| c == '{').count();
            let close = content.chars().filter(|&c| c == '}').count();
            if open > close {
                let mut fixed = content.to_string();
                for _ in 0..(open - close) { fixed.push_str("
}"); }
                std::borrow::Cow::Owned(fixed)
            } else {
                std::borrow::Cow::Borrowed(content)
            }
        } else {
            std::borrow::Cow::Borrowed(content)
        };
        std::fs::write(&p, content.as_ref())?;
        println!("   📝 {} ({} bytes)", path, content.len());
        Ok(ExecResult::ok(format!("Written: {}", path)))
    }

    fn append_file(&self, path: &str, content: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        if !p.exists() {
            return Ok(ExecResult::fail(format!("'{}' not found — use write_file first", path)));
        }
        let mut orig = std::fs::read_to_string(&p)?;
        if !orig.ends_with('\n') { orig.push('\n'); }
        orig.push('\n');
        orig.push_str(content);
        std::fs::write(&p, &orig)?;
        println!("   ➕ {} (+{} bytes)", path, content.len());
        Ok(ExecResult::ok(format!("Appended: {}", path)))
    }

    fn read_file(&self, path: &str) -> Result<ExecResult> {
        let p = self.safe_path(path)?;
        if !p.exists() {
            return Ok(ExecResult::fail(format!("'{}' not found", path)));
        }
        let content = std::fs::read_to_string(&p)?;
        println!("   📖 {} ({} bytes)", path, content.len());
        Ok(ExecResult { success: true, exit_code: 0, stdout: content, stderr: String::new(), duration_ms: 0 })
    }

    fn mkdir(&self, path: &str) -> Result<ExecResult> {
        std::fs::create_dir_all(self.workspace.join(path))?;
        Ok(ExecResult::ok(format!("mkdir: {}", path)))
    }

    // ─── RunTests ──────────────────────────────────

    async fn run_tests(&self, target: &str) -> Result<ExecResult> {
        // Rust tests
        if target.ends_with(".rs") || target == "cargo" {
            println!("   🦀 cargo test");
            let start = std::time::Instant::now();
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new("cargo")
                    .args(["test", "--", "--nocapture"])
                    .current_dir(&self.workspace)
                    .output(),
            ).await
            .map_err(|_| anyhow!("cargo test timeout"))??;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{}\n{}", stdout, stderr);
            // SHA2 Digest Auto-fix v1.4
            if combined.contains("trait `Digest` which provides") {
                println!("   🔧 AutoFix: adding sha2::Digest import");
                for dir in [self.workspace.as_path(), self.workspace.join("src").as_path()] {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                                if let Ok(content) = std::fs::read_to_string(&p) {
                                    if content.contains("sha2::Sha256") && !content.contains("use sha2::Digest") {
                                        let _ = std::fs::write(&p, format!("use sha2::Digest;\n{}", content));
                                        println!("   ✅ Fixed {:?}", p.file_name().unwrap_or_default());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let success = out.status.success();
            if success { println!("   ✅ Tests passed (exit 0)"); }
            else       { println!("   ❌ Tests FAILED (exit {})", out.status.code().unwrap_or(-1)); }
            let (passed, failed) = parse_rust_tests(&combined);
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success { String::new() } else {
                    let start = combined.len().saturating_sub(2000);
                    combined[start..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // Go tests
        if target.ends_with(".go") || target == "go" {
            println!("   🐹 go test ./...");
            // Auto-fix: run go mod tidy before tests if go.mod exists
            if self.workspace.join("go.mod").exists() {
                let _ = TCmd::new("go")
                    .args(["mod", "tidy"])
                    .current_dir(&self.workspace)
                    .output().await;
                println!("   🔧 AutoFix: go mod tidy done");
            }
            let start = std::time::Instant::now();
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new("go")
                    .args(["test", "./...", "-v"])
                    .current_dir(&self.workspace)
                    .output(),
            ).await
            .map_err(|_| anyhow!("go test timeout"))??;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{}\n{}", stdout, stderr);
            let success = out.status.success();
            if success { println!("   ✅ Tests passed (exit 0)"); }
            else       { println!("   ❌ Tests FAILED (exit {})", out.status.code().unwrap_or(-1)); }
            let (passed, failed) = parse_go_tests(&combined);
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed", passed, failed),
                stderr: if success { String::new() } else {
                    let start = combined.len().saturating_sub(2000);
                    combined[start..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        // Node.js tests — npm test / jest / npx jest
        let is_node_target = target.ends_with(".js")
            || target == "npm test"
            || target == "npm"
            || target == "jest"
            || target == "npx jest"
            || target.contains("jest");
        if is_node_target {
            // تحديد الأمر الصحيح
            let (prog, args): (&str, Vec<&str>) = if target == "jest" {
                // شغّل jest مباشرة من node_modules
                ("npx", vec!["jest", "--runInBand", "--forceExit"])
            } else if target == "npx jest" {
                ("npx", vec!["jest", "--runInBand", "--forceExit"])
            } else if target == "npm test" || target == "npm" {
                ("npm", vec!["test", "--", "--runInBand", "--forceExit"])
            } else {
                // .js file — شغّل مع node
                ("node", vec![target])
            };
            println!("   🟨 {} {}", prog, args.join(" "));
            let start = std::time::Instant::now();
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(self.timeout_secs),
                TCmd::new(prog).args(&args).current_dir(&self.workspace).output(),
            ).await
            .map_err(|_| anyhow!("Node.js test timeout after {}s", self.timeout_secs))??;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{}
{}", stdout, stderr);
            let success = out.status.success();
            if success { println!("   ✅ Tests passed (exit 0)"); }
            else       { println!("   ❌ Tests FAILED (exit {})", out.status.code().unwrap_or(-1)); }
            // parse Jest results
            let passed = combined.lines()
                .filter(|l| l.contains("✓") || l.contains("✔") || l.contains("passed"))
                .count();
            let failed = combined.lines()
                .filter(|l| l.contains("✗") || l.contains("✘") || l.contains("failed") || l.contains("FAIL"))
                .count();
            return Ok(ExecResult {
                success,
                exit_code: out.status.code().unwrap_or(-1),
                stdout: format!("{} passed, {} failed
{}", passed, failed, &stdout[..stdout.len().min(1000)]),
                stderr: if success { String::new() } else {
                    let start = combined.len().saturating_sub(2000);
                    combined[start..].to_string()
                },
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        let pytest = if self.workspace.join("venv/bin/pytest").exists() {
            "venv/bin/pytest"
        } else { "pytest" };


        // StdlibConflict Pre-check v1.3
        {
            let conflicts = ["numbers","decimal","types","typing","abc","queue",
                             "math","string","io","re","json","csv",
                             "random","time","collections","functools",
                             "itertools","pathlib","enum","copy"];
            for name in &conflicts {
                let f = self.workspace.join(format!("{}.py", name));
                if f.exists() {
                    println!("   Removing {}.py (stdlib conflict)", name);
                    let _ = std::fs::remove_file(&f);
                }
            }
        }
        let t = if target.is_empty() || target == "." { String::new() } else { format!(" {}", target) };
        let cmd = format!("{}{} -v --tb=short", pytest, t);
        println!("   🧪 {}", cmd);

        let out = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            TCmd::new(pytest)
                .args(if target.is_empty() || target == "." { vec!["-v", "--tb=short"] } else { vec![target, "-v", "--tb=short"] })
                .current_dir(&self.workspace)
                .env("PYTHONPATH", &self.workspace)
                .output(),
        ).await
        .map_err(|_| anyhow!("pytest timeout"))??;

        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let combined = format!("{}\n{}", stdout, stderr);

        let (passed, failed) = parse_pytest(&combined);

        // Smart Success Detection v1.3
        // exit code وحده غير كافٍ — بعض الأدوات (PyQt6-WebEngine) تُرجع -1 مع اختبارات ناجحة
        // القاعدة الآمنة: نجاح فقط إذا passed>0 و failed==0 و لا يوجد error في الـ summary
        let has_passed  = passed > 0;
        let has_failed  = failed > 0;
        let has_error   = combined.contains("ERROR collecting")
                       || combined.contains("error during collection")
                       || combined.contains("errors in collection");
        let collected_zero = combined.contains("collected 0 items")
                          || combined.contains("no tests ran");

        let success = if has_passed && !has_failed && !has_error && !collected_zero {
            // الاختبارات نجحت — تجاهل exit code غير الصفري من أدوات خارجية
            if out.status.code().unwrap_or(0) != 0 && out.status.code() != Some(5) {
                println!("   ⚠ exit code {} — ignored (tests passed cleanly)", out.status.code().unwrap_or(-1));
            }
            true
        } else {
            // لا توجد passed أو يوجد فشل — اعتمد exit code
            out.status.success() && out.status.code() != Some(5)
        };

        if success { println!("   ✅ Tests passed (exit 0)"); }
        else       { println!("   ❌ Tests FAILED (exit {})", out.status.code().unwrap_or(-1)); }

        Ok(ExecResult {
            success,
            exit_code:   out.status.code().unwrap_or(-1),
            stdout:      format!("{} passed, {} failed", passed, failed),
            stderr:      if success {
                String::new()
            } else {
                let tail_start = combined.len().saturating_sub(2000);
                combined[tail_start..].to_string()
            },
            duration_ms: 0,
        })
    }

    // ─── Helpers ───────────────────────────────────

    fn safe_path(&self, path: &str) -> Result<PathBuf> {
        if path.contains("..") {
            return Err(anyhow!(SafetyError::PathTraversal(path.to_string()).to_string()));
        }
        let full = self.workspace.join(path);
        if !full.starts_with(&self.workspace) {
            return Err(anyhow!(SafetyError::WorkspaceEscape(path.to_string()).to_string()));
        }
        Ok(full)
    }

    fn safety_check(&self, cmd: &str) -> Result<()> {
        let lower = cmd.to_lowercase();
        for b in BLOCKED {
            if lower.contains(b) {
                return Err(anyhow!(SafetyError::BlockedCommand(b.to_string()).to_string()));
            }
        }
        Ok(())
    }
}

fn parse_pytest(output: &str) -> (usize, usize) {
    let mut passed = 0;
    let mut failed = 0;
    for line in output.lines().rev() {
        if line.contains(" passed") || line.contains(" failed") {
            // السطر: "=== 2 passed, 1 failed in 0.03s ==="
            // نبحث عن الرقم قبل كل كلمة مفتاحية
            for seg in line.split(',') {
                let s = seg.trim();
                let words: Vec<&str> = s.split_whitespace().collect();
                for (i, w) in words.iter().enumerate() {
                    if *w == "passed" && i > 0 {
                        if let Ok(n) = words[i-1].parse::<usize>() { passed = n; }
                    }
                    if *w == "failed" && i > 0 {
                        if let Ok(n) = words[i-1].parse::<usize>() { failed = n; }
                    }
                }
            }
            break;
        }
    }
    (passed, failed)
}


#[derive(Debug, PartialEq)]
pub enum MutationResult {
    Strong,
    Weak(String, String),  // (original_line, mutated_line)
    Skipped,
}

fn apply_all_mutations(code: &str) -> Vec<(String, String, String)> {
    let strategies: &[(&str, &str)] = &[
        ("==", "!="), ("!=", "=="),
        (" > ", " < "), (" < ", " > "),
        (" >= ", " <= "), (" <= ", " >= "),
        ("return True", "return False"), ("return False", "return True"),
        (" + ", " - "), (" - ", " + "),
    ];
    let skip_patterns = ["i += ", "i -= ", "j += ", "j -= ",
                          "idx", "index", "len(", "range(", "count +=", "count -="];
    let mut result = Vec::new();
    for (from, to) in strategies {
        let mut found_line = None;
        let mutated: String = code.lines()
            .map(|line| {
                let trimmed = line.trim_start();
                let skip = trimmed.starts_with('#') || trimmed.starts_with("//")
                        || skip_patterns.iter().any(|p| line.contains(p));
                if found_line.is_none() && !skip && line.contains(*from) {
                    let new_line = line.replacen(from, to, 1);
                    found_line = Some((line.trim().to_string(), new_line.trim().to_string()));
                    new_line
                } else { line.to_string() }
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
        if !source_path.exists() { return MutationResult::Skipped; }
        let ext = source_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "py" { return MutationResult::Skipped; }
        let original = match std::fs::read_to_string(&source_path) {
            Ok(s) => s, Err(_) => return MutationResult::Skipped,
        };
        let mutations = apply_all_mutations(&original);
        if mutations.is_empty() { return MutationResult::Skipped; }
        let pytest = if self.workspace.join("venv/bin/pytest").exists() {
            "venv/bin/pytest"
        } else { "pytest" };
        let mut survived_orig = String::new();
        let mut survived_mutd = String::new();
        let mut any_caught  = false;
        let mut any_missed  = false;
        for (mutation, orig_line, mutd_line) in &mutations {
            if std::fs::write(&source_path, mutation).is_err() {
                let _ = std::fs::write(&source_path, &original);
                continue;
            }
            let out = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                tokio::process::Command::new(pytest)
                    .args(["-x", "-q", "--tb=no"])
                    .current_dir(&self.workspace)
                    .env("PYTHONPATH", &self.workspace)
                    .output(),
            ).await;
            let _ = std::fs::write(&source_path, &original);
            match out {
                Ok(Ok(result)) => {
                    if result.status.success() {
                        if !any_missed {
                            survived_orig = orig_line.clone();
                            survived_mutd = mutd_line.clone();
                        }
                        any_missed = true;
                    } else { any_caught = true; }
                }
                _ => {}
            }
            if any_caught { break; }
        }
        let _ = std::fs::write(&source_path, &original);
        if any_caught { MutationResult::Strong }
        else if any_missed { MutationResult::Weak(survived_orig, survived_mutd) }
        else { MutationResult::Skipped }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn ex(dir: &std::path::Path) -> SafeExecutor {
        SafeExecutor::new(dir.to_path_buf(), 10)
    }

    #[test]
    fn write_and_read() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        e.write_file("a.py", "x=1").unwrap();
        let r = e.read_file("a.py").unwrap();
        assert_eq!(r.stdout, "x=1");
    }

    #[test]
    fn append_file() {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join("a.py"), "line1\n").unwrap();
        let e = ex(d.path());
        let r = e.append_file("a.py", "line2").unwrap();
        assert!(r.success);
        let c = std::fs::read_to_string(d.path().join("a.py")).unwrap();
        assert!(c.contains("line1") && c.contains("line2"));
    }

    #[test]
    fn blocks_path_traversal() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        assert!(e.write_file("../../etc/passwd", "x").is_err());
    }

    #[tokio::test]
    async fn runs_echo() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        let r = e.run(&Cmd::Run { command: "echo hello".into() }).await.unwrap();
        assert!(r.success);
        assert!(r.stdout.contains("hello"));
    }
}

fn parse_rust_tests(output: &str) -> (usize, usize) {
    for line in output.lines() {
        if line.contains("test result:") {
            let mut passed = 0usize;
            let mut failed = 0usize;
            for seg in line.split(';') {
                let s = seg.trim();
                let words: Vec<&str> = s.split_whitespace().collect();
                for (i, w) in words.iter().enumerate() {
                    if *w == "passed" && i > 0 {
                        if let Ok(n) = words[i-1].parse() { passed = n; }
                    }
                    if *w == "failed" && i > 0 {
                        if let Ok(n) = words[i-1].parse() { failed = n; }
                    }
                }
            }
            return (passed, failed);
        }
    }
    (0, 0)
}

fn parse_go_tests(output: &str) -> (usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    for line in output.lines() {
        if line.starts_with("--- PASS") { passed += 1; }
        if line.starts_with("--- FAIL") { failed += 1; }
    }
    (passed, failed)
}
