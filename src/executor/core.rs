use crate::protocol::Cmd;
use crate::types::ExecResult;
use crate::workspace_oracle::WorkspaceOracle;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::PathBuf;
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
    pub patch_attempts: std::cell::RefCell<HashMap<PathBuf, usize>>, // v5.2: track patch failures
}

impl SafeExecutor {
    pub fn new(workspace: PathBuf, timeout_secs: u64) -> Self {
        let oracle = WorkspaceOracle::new(workspace.clone());
        Self {
            workspace,
            oracle,
            timeout_secs,
            replay_mode: false,
            patch_attempts: std::cell::RefCell::new(HashMap::new()),
        }
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

        let parts: Vec<&str> = command.split_whitespace().collect();
        let prog = parts.first().ok_or_else(|| anyhow!("Empty command"))?;

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
    pub fn write_and_read() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        e.write_file("a.py", "x=1").unwrap();
        let r = e.read_file("a.py").unwrap();
        assert_eq!(r.stdout, "x=1");
    }

    #[test]
    pub fn append_file() {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join("a.py"), "line1\n").unwrap();
        let e = ex(d.path());
        let r = e.append_file("a.py", "line2").unwrap();
        assert!(r.success);
        let c = std::fs::read_to_string(d.path().join("a.py")).unwrap();
        assert!(c.contains("line1") && c.contains("line2"));
    }

    #[test]
    pub fn blocks_path_traversal() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        assert!(e.write_file("../../etc/passwd", "x").is_err());
    }

    #[tokio::test]
    pub async fn runs_echo() {
        let d = tempdir().unwrap();
        let e = ex(d.path());
        let r = e
            .run(&Cmd::Run {
                command: "echo hello".into(),
            })
            .await
            .unwrap();
        assert!(r.success);
        assert!(r.stdout.contains("hello"));
    }
}
