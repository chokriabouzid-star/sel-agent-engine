use crate::protocol::Cmd;
use crate::types::ExecResult;
use crate::workspace_oracle::WorkspaceOracle;
use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tokio::process::Command as TCmd;

pub const ALLOWED: &[&str] = &[
    "python3",
    "python",
    "venv/bin/python3",
    "venv/bin/python",
    "venv/bin/pip3",
    "venv/bin/pip",
    "venv/bin/uvicorn",
    "venv/bin/gunicorn",
    "venv/bin/pytest",
    "pytest",
    "node",
    "npm",
    "npx",
    "node_modules/.bin/jest",
    "cargo",
    "rustc",
    "git",
    "go",
    "mkdir",
    "touch",
    "ls",
    "cat",
    "cp",
    "mv",
    "echo",
    "find",
    "grep",
    "curl",
    "chmod",
    "node",
    "npm",
];

pub const BLOCKED: &[&str] = &[
    "sudo",
    "rm -rf",
    "mkfs",
    "dd if=",
    "| sh",
    "| bash",
    "curl | bash",
    "> /dev/",
    "/etc/",
    "/sys/",
    "/proc/",
];

pub struct SafeExecutor {
    pub workspace: PathBuf,
    pub oracle: WorkspaceOracle,
    pub timeout_secs: u64,
    pub replay_mode: bool, // v7.6.1: prevents internet access during replay
    pub bench_mode: bool,  // v8.5: run-mode hint
    pub protected_test_files: HashSet<PathBuf>, // tests present before agent writes anything
    pub patch_attempts: std::cell::RefCell<HashMap<PathBuf, usize>>, // v5.2: track patch failures
    goal_authorized_test_files: RwLock<HashSet<PathBuf>>,
    allow_goal_test_writes: AtomicBool,
    broken_authorized_test_repair: AtomicBool,
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r#"'\''"#))
}

fn normalize_shell_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let inner = trimmed
        .strip_prefix("print(")
        .and_then(|s| s.strip_suffix(')'))?;

    let payload = inner.trim();
    if payload.len() >= 2
        && ((payload.starts_with('\'') && payload.ends_with('\''))
            || (payload.starts_with('"') && payload.ends_with('"')))
    {
        let message = &payload[1..payload.len() - 1];
        return Some(format!("echo {}", shell_single_quote(message)));
    }

    None
}

impl SafeExecutor {
    pub fn new(workspace: PathBuf, timeout_secs: u64) -> Self {
        let oracle = WorkspaceOracle::new(workspace.clone());
        Self {
            workspace,
            oracle,
            timeout_secs,
            replay_mode: false,
            bench_mode: false,
            protected_test_files: HashSet::new(),
            patch_attempts: std::cell::RefCell::new(HashMap::new()),
            goal_authorized_test_files: RwLock::new(HashSet::new()),
            allow_goal_test_writes: AtomicBool::new(false),
            broken_authorized_test_repair: AtomicBool::new(false),
        }
    }

    pub(crate) fn set_goal_authorized_test_files(&self, paths: &[PathBuf]) {
        let mut guard = self
            .goal_authorized_test_files
            .write()
            .expect("goal-authorized test files lock poisoned");
        guard.clear();
        guard.extend(paths.iter().cloned());
    }

    pub(crate) fn clear_goal_authorized_test_files(&self) {
        let mut guard = self
            .goal_authorized_test_files
            .write()
            .expect("goal-authorized test files lock poisoned");
        guard.clear();
    }

    pub(crate) fn set_allow_goal_test_writes(&self, allow: bool) {
        self.allow_goal_test_writes.store(allow, Ordering::Relaxed);
    }

    pub(crate) fn set_broken_authorized_test_repair(&self, allow: bool) {
        self.broken_authorized_test_repair
            .store(allow, Ordering::Relaxed);
    }

    pub(crate) fn goal_authorized_test_write_allowed(&self, path: &std::path::Path) -> bool {
        let guard = self
            .goal_authorized_test_files
            .read()
            .expect("goal-authorized test files lock poisoned");

        if !guard.contains(path) {
            return false;
        }

        if self.allow_goal_test_writes.load(Ordering::Relaxed) {
            return true;
        }

        self.broken_authorized_test_repair.load(Ordering::Relaxed)
    }

    pub async fn run(&self, cmd: &Cmd) -> Result<ExecResult> {
        match cmd {
            Cmd::Run { command } => self.shell(command).await,
            Cmd::WriteFile { path, content } => self.write_file(path, content),
            Cmd::AppendFile { path, content } => self.append_file(path, content),
            Cmd::DeleteFile { path } => self.delete_file(path),
            Cmd::PatchFile {
                path,
                search,
                replace,
            } => self.patch_file(path, search, replace),
            Cmd::ReadFile { path } => self.read_file(path),
            Cmd::Mkdir { path } => self.mkdir(path),
            Cmd::RunTests { target } => self.run_tests(target).await,
            Cmd::Done { .. } => Ok(ExecResult::ok("done")),
        }
    }

    //  Shell

    async fn shell(&self, command: &str) -> Result<ExecResult> {
        self.safety_check(command)?;

        let normalized_command = normalize_shell_command(command);
        let command = if let Some(ref normalized) = normalized_command {
            eprintln!(
                "   ⚡ AutoFix: normalized shell command '{}' -> '{}'",
                command, normalized
            );
            normalized.as_str()
        } else {
            command
        };

        let parts: Vec<&str> = command.split_whitespace().collect();
        let prog = parts.first().ok_or_else(|| anyhow!("Empty command"))?;

        if self.replay_mode && *prog == "npm" {
            let action = parts.get(1).copied().unwrap_or("");
            let is_mutating_npm = matches!(action, "install" | "i" | "ci");

            if is_mutating_npm {
                let node_modules = self.workspace.join("node_modules");
                if node_modules.exists() {
                    eprintln!(
                        "[TRACE] Replay mode: skipping '{}' to preserve cached node_modules",
                        command
                    );
                    return Ok(ExecResult::ok(
                        "Replay mode: skipped npm dependency mutation; cached node_modules present",
                    ));
                } else {
                    return Ok(ExecResult::fail(
                        "REPLAY_ENV_MISMATCH: npm dependency install requested in replay but node_modules is unavailable".to_string(),
                    ));
                }
            }
        }

        //  pip install  package name
        if prog.contains("pip3") || prog.contains("pip") {
            let is_install = parts.contains(&"install");
            let has_package = parts.len() > 2 && parts.iter().skip(2).any(|p| !p.starts_with('-'));
            if is_install && !has_package {
                return Ok(ExecResult::fail(
                    "pip install needs package name: e.g. venv/bin/pip3 install pytest".to_string(),
                ));
            }
        }

        // v8.4: Allow workspace-local binaries (./main, ./server, target/debug/*)
        let is_local_binary = prog.starts_with("./") || prog.starts_with("target/");
        if !is_local_binary && !ALLOWED.contains(prog) {
            return Ok(ExecResult::fail(format!(
                "'{}' is not in the allowed programs list",
                prog
            )));
        }

        let services = ["venv/bin/uvicorn", "uvicorn", "venv/bin/gunicorn"];
        if services.contains(prog) {
            return self.service(prog, &parts[1..]).await;
        }

        let start = Instant::now();
        let out = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            TCmd::new(prog)
                .args(&parts[1..])
                .current_dir(&self.workspace)
                .output(),
        )
        .await
        .map_err(|_| anyhow!("Timeout after {}s: {}", self.timeout_secs, command))??;

        Ok(ExecResult {
            success: out.status.success(),
            exit_code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into(),
            stderr: String::from_utf8_lossy(&out.stderr).into(),
            duration_ms: start.elapsed().as_millis() as u64,
            autofix_triggered: false,
        })
    }

    async fn service(&self, prog: &str, args: &[&str]) -> Result<ExecResult> {
        println!("   🚀 Service: {}", prog);
        TCmd::new(prog)
            .args(args)
            .current_dir(&self.workspace)
            .spawn()?;
        tokio::time::sleep(Duration::from_millis(800)).await;
        Ok(ExecResult::ok("Service started"))
    }

    pub fn safe_path(&self, path: &str) -> Result<PathBuf> {
        if path.contains("..") {
            return Err(anyhow!("Path traversal detected"));
        }
        let full = self.workspace.join(path);
        if !full.starts_with(&self.workspace) {
            return Err(anyhow!("Workspace escape detected"));
        }
        Ok(full)
    }

    pub fn safety_check(&self, cmd: &str) -> Result<()> {
        if let Err(e) = crate::constitution::check_command(cmd) {
            return Err(anyhow!(e.to_string()));
        }

        let lower = cmd.to_lowercase();
        for b in BLOCKED {
            if lower.contains(b) {
                return Err(anyhow!("Blocked command detected: {}", b));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    pub fn ex(dir: &std::path::Path) -> SafeExecutor {
        SafeExecutor::new(dir.to_path_buf(), 10)
    }

    #[test]
    pub fn goal_authorized_test_policy_requires_membership_and_window() {
        let d = tempdir().expect("test setup/use should succeed");
        let e = ex(d.path());
        let test_file = d.path().join("main_test.go");

        e.set_goal_authorized_test_files(std::slice::from_ref(&test_file));
        assert!(!e.goal_authorized_test_write_allowed(&test_file));

        e.set_allow_goal_test_writes(true);
        assert!(e.goal_authorized_test_write_allowed(&test_file));

        e.set_allow_goal_test_writes(false);
        e.set_broken_authorized_test_repair(true);
        assert!(e.goal_authorized_test_write_allowed(&test_file));

        e.set_broken_authorized_test_repair(false);
        e.clear_goal_authorized_test_files();
        assert!(!e.goal_authorized_test_write_allowed(&test_file));
    }

    #[test]
    pub fn write_and_read() {
        let d = tempdir().expect("test setup/use should succeed");
        let e = ex(d.path());
        e.write_file("a.py", "x=1")
            .expect("test setup/use should succeed");
        let r = e.read_file("a.py").expect("test setup/use should succeed");
        assert_eq!(r.stdout, "x=1");
    }

    #[test]
    pub fn append_file() {
        let d = tempdir().expect("test setup/use should succeed");
        std::fs::write(d.path().join("a.py"), "line1\n").expect("test setup/use should succeed");
        let e = ex(d.path());
        let r = e
            .append_file("a.py", "line2")
            .expect("test setup/use should succeed");
        assert!(r.success);
        let c =
            std::fs::read_to_string(d.path().join("a.py")).expect("test setup/use should succeed");
        assert!(c.contains("line1") && c.contains("line2"));
    }

    #[test]
    pub fn blocks_path_traversal() {
        let d = tempdir().expect("test setup/use should succeed");
        let e = ex(d.path());
        assert!(e.write_file("../../etc/passwd", "x").is_err());
    }

    #[tokio::test]
    pub async fn runs_echo() {
        let d = tempdir().expect("test setup/use should succeed");
        let e = ex(d.path());
        let r = e
            .run(&Cmd::Run {
                command: "echo hello".into(),
            })
            .await
            .expect("test setup/use should succeed");
        assert!(r.success);
        assert!(r.stdout.contains("hello"));
    }

    #[tokio::test]
    pub async fn replay_skips_npm_install_when_node_modules_exists() {
        let d = tempdir().expect("test setup/use should succeed");
        std::fs::create_dir_all(d.path().join("node_modules"))
            .expect("test setup/use should succeed");

        let mut e = ex(d.path());
        e.replay_mode = true;

        let r = e
            .run(&Cmd::Run {
                command: "npm install crypto".into(),
            })
            .await
            .expect("test setup/use should succeed");

        assert!(r.success);
        assert!(
            r.stdout.contains("skipped npm dependency mutation")
                || r.stderr.contains("skipped npm dependency mutation")
        );
    }

    #[tokio::test]
    pub async fn replay_rejects_npm_install_without_node_modules() {
        let d = tempdir().expect("test setup/use should succeed");

        let mut e = ex(d.path());
        e.replay_mode = true;

        let r = e
            .run(&Cmd::Run {
                command: "npm install crypto".into(),
            })
            .await
            .expect("test setup/use should succeed");

        assert!(!r.success);
        assert!(
            r.stdout.contains("REPLAY_ENV_MISMATCH") || r.stderr.contains("REPLAY_ENV_MISMATCH")
        );
    }

    #[test]
    pub fn blocks_constitution_network_command() {
        let d = tempdir().expect("test setup/use should succeed");
        let e = ex(d.path());
        assert!(e.safety_check("curl https://example.com").is_err());
    }

    #[test]
    pub fn allows_safe_test_command() {
        let d = tempdir().expect("test setup/use should succeed");
        let e = ex(d.path());
        assert!(e.safety_check("cargo test").is_ok());
    }
}
