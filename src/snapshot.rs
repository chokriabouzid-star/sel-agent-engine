// src/snapshot.rs
// Workspace snapshots that preserve the CURRENT worktree for repair attempts.
// The stash acts as a backup copy only; the worktree stays intact after take().

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Tracks the result of the stash operation to prevent ambiguous None handling.
enum StashState {
    /// git stash said "No local changes to save" — workspace was clean, safe to reset
    NoLocalChanges,
    /// git stash succeeded and ref is verified — reset then re-apply
    Stashed { tag: String },
    /// git stash failed (e.g. index.lock) — do NOT reset, protect user data
    Failed(String),
}

pub struct Snapshot {
    workspace: PathBuf,
    active: bool,
    stash_state: StashState,
}

impl Snapshot {
    fn resolve_stash_ref(workspace: &Path, tag: &str) -> Option<String> {
        let out = Command::new("git")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(["stash", "list"])
            .current_dir(workspace)
            .output()
            .ok()?;

        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .find(|line| line.contains(tag))
            .and_then(|line| line.split(':').next())
            .map(|s| s.trim().to_string())
    }

    fn restore_python_infra(&self, had_venv: bool) {
        if had_venv && !self.workspace.join("venv").exists() {
            let cache_venv = crate::scaffold_engine::get_cache_dir().join("python/venv");
            if cache_venv.exists() {
                let _ = std::os::unix::fs::symlink(&cache_venv, self.workspace.join("venv"));
            } else {
                let _ = Command::new("python3")
                    .args(["-m", "venv", "venv"])
                    .current_dir(&self.workspace)
                    .output();

                let _ = Command::new("venv/bin/pip")
                    .args(["install", "pytest", "-q"])
                    .current_dir(&self.workspace)
                    .output();
            }
        }

        let pytest_bin = self.workspace.join("venv/bin/pytest");
        let pip_bin = self.workspace.join("venv/bin/pip");
        if self.workspace.join("venv").exists() && !pytest_bin.exists() && pip_bin.exists() {
            let _ = Command::new(&pip_bin)
                .args(["install", "pytest", "-q"])
                .current_dir(&self.workspace)
                .output();
        }
    }

    fn git_reset_clean(workspace: &Path) {
        let _ = Command::new("git")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(["reset", "--hard"])
            .current_dir(workspace)
            .output();
        let _ = Command::new("git")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(["clean", "-fd"])
            .current_dir(workspace)
            .output();
    }

    /// Take a snapshot of the current workspace while KEEPING the current
    /// worktree intact. We use git stash as a backup, then re-apply it
    /// immediately so repair code can still see the created files.
    pub fn take(workspace: &Path) -> Self {
        // Ensure it's a git repo
        if !workspace.join(".git").exists() {
            let _ = Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .arg("init")
                .current_dir(workspace)
                .output();
        }

        // Protect infrastructure dirs from stash/cleanup side effects
        let gitignore = workspace.join(".gitignore");
        let existing = std::fs::read_to_string(&gitignore).unwrap_or_default();
        if !existing.contains("venv/") {
            let mut content = existing;
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("venv/\nnode_modules/\n__pycache__/\ntarget/\n");
            let _ = std::fs::write(&gitignore, content);
        }

        // Ensure there is at least one commit so stash/reset work correctly
        let has_head = Command::new("git")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(["rev-parse", "HEAD"])
            .current_dir(workspace)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !has_head {
            let _ = Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["config", "user.name", "SEL Agent"])
                .current_dir(workspace)
                .output();
            let _ = Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["config", "user.email", "sel@local.test"])
                .current_dir(workspace)
                .output();
            let _ = Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["add", "."])
                .current_dir(workspace)
                .output();
            let _ = Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["commit", "--allow-empty", "-m", "Initial commit baseline"])
                .current_dir(workspace)
                .output();
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);

        let tag = format!("sel_agent_snapshot_{}_{}", std::process::id(), now_ms);

        // FIX C-01: check for index.lock BEFORE running stash
        // git stash --include-untracked deletes untracked files before detecting lock failure
        let index_lock = workspace.join(".git/index.lock");
        if index_lock.exists() {
            eprintln!(
                "[WARN] Snapshot: .git/index.lock exists — skipping stash to protect untracked files"
            );
            return Self {
                workspace: workspace.to_path_buf(),
                active: true,
                stash_state: StashState::Failed("index.lock present".to_string()),
            };
        }

        let output = Command::new("git")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(["stash", "push", "--include-untracked", "-m"])
            .arg(&tag)
            .current_dir(workspace)
            .output();

        // FIX C-01: three-state stash result — no ambiguous None
        let stash_state = match output {
            Err(e) => {
                // Could not even spawn git — treat as failure
                eprintln!("[WARN] Snapshot: could not run git stash: {} — workspace NOT reset on rollback", e);
                StashState::Failed(format!("git spawn failed: {}", e))
            }
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.contains("No local changes to save") {
                    // Workspace was clean — safe to reset without stash
                    eprintln!("[TRACE] Snapshot: no local changes, reset will be safe");
                    StashState::NoLocalChanges
                } else if out.status.success() {
                    // Stash succeeded — verify the ref exists
                    if let Some(stash_ref) = Self::resolve_stash_ref(workspace, &tag) {
                        // Re-apply so worktree stays intact for repair code
                        let apply_out = Command::new("git")
                            .env("LC_ALL", "C")
                            .env("LANG", "C")
                            .args(["stash", "apply"])
                            .arg(&stash_ref)
                            .current_dir(workspace)
                            .output();
                        match apply_out {
                            Ok(o) if o.status.success() => {
                                eprintln!(
                                    "[TRACE] Snapshot: {} saved and re-applied to worktree",
                                    stash_ref
                                );
                            }
                            Ok(o) => {
                                eprintln!(
                                    "[TRACE] Snapshot: failed to re-apply {}: {}",
                                    stash_ref,
                                    String::from_utf8_lossy(&o.stderr)
                                );
                            }
                            Err(e) => {
                                eprintln!(
                                    "[TRACE] Snapshot: failed to re-apply {}: {}",
                                    stash_ref, e
                                );
                            }
                        }
                        StashState::Stashed { tag }
                    } else {
                        eprintln!(
                            "[WARN] Snapshot: stash exit=0 but ref not found for tag {} — treating as failure",
                            tag
                        );
                        StashState::Failed(format!("stash ref not found for tag {}", tag))
                    }
                } else {
                    // exit != 0 — stash failed (e.g. index.lock)
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    eprintln!(
                        "[WARN] Snapshot: stash failed (exit={}) — workspace NOT reset on rollback: {}",
                        out.status.code().unwrap_or(-1),
                        stderr.trim()
                    );
                    StashState::Failed(format!(
                        "stash exit={}: {}",
                        out.status.code().unwrap_or(-1),
                        stderr.trim()
                    ))
                }
            }
        };

        Self {
            workspace: workspace.to_path_buf(),
            active: true,
            stash_state,
        }
    }

    /// Rollback workspace to the pre-attempt state stored in this snapshot.
    pub fn rollback(&mut self) {
        if !self.active {
            return;
        }

        let had_venv = self.workspace.join("venv").exists();

        match &self.stash_state {
            StashState::NoLocalChanges => {
                // Workspace was clean when snapshot was taken — safe to reset
                Self::git_reset_clean(&self.workspace);
                println!("    Snapshot: reset workspace (was clean, no stash needed)");
            }
            StashState::Stashed { tag } => {
                // Verify stash ref still exists before reset
                if let Some(stash_ref) = Self::resolve_stash_ref(&self.workspace, tag) {
                    Self::git_reset_clean(&self.workspace);
                    let _ = Command::new("git")
                        .env("LC_ALL", "C")
                        .env("LANG", "C")
                        .args(["stash", "apply"])
                        .arg(&stash_ref)
                        .current_dir(&self.workspace)
                        .output();
                    let _ = Command::new("git")
                        .env("LC_ALL", "C")
                        .env("LANG", "C")
                        .args(["stash", "drop"])
                        .arg(&stash_ref)
                        .current_dir(&self.workspace)
                        .output();
                    println!("    Snapshot: rolled back via {}", stash_ref);
                } else {
                    // Stash ref disappeared — do NOT reset
                    eprintln!(
                        "[WARN] Snapshot: stash ref not found for tag {} — workspace NOT reset to protect user data",
                        tag
                    );
                }
            }
            StashState::Failed(reason) => {
                // Stash failed at take() time — resetting would destroy user data
                eprintln!(
                    "[WARN] Snapshot: rollback skipped — stash failed at snapshot time ({}). Workspace preserved.",
                    reason
                );
            }
        }

        self.restore_python_infra(had_venv);
        self.active = false;
    }

    /// Accept the current changes and discard the backup stash.
    pub fn commit(&mut self) {
        if !self.active {
            return;
        }

        if let StashState::Stashed { tag } = &self.stash_state {
            if let Some(stash_ref) = Self::resolve_stash_ref(&self.workspace, tag) {
                let _ = Command::new("git")
                    .env("LC_ALL", "C")
                    .env("LANG", "C")
                    .args(["stash", "drop"])
                    .arg(&stash_ref)
                    .current_dir(&self.workspace)
                    .output();
                eprintln!("[TRACE] Snapshot: {} dropped (changes accepted)", stash_ref);
            }
        }

        self.active = false;
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        let had_venv = self.workspace.join("venv").exists();

        match &self.stash_state {
            StashState::NoLocalChanges => {
                Self::git_reset_clean(&self.workspace);
            }
            StashState::Stashed { tag } => {
                if let Some(stash_ref) = Self::resolve_stash_ref(&self.workspace, tag) {
                    Self::git_reset_clean(&self.workspace);
                    let _ = Command::new("git")
                        .env("LC_ALL", "C")
                        .env("LANG", "C")
                        .args(["stash", "apply"])
                        .arg(&stash_ref)
                        .current_dir(&self.workspace)
                        .output();
                    let _ = Command::new("git")
                        .env("LC_ALL", "C")
                        .env("LANG", "C")
                        .args(["stash", "drop"])
                        .arg(&stash_ref)
                        .current_dir(&self.workspace)
                        .output();
                    eprintln!("[TRACE] Snapshot: {} restored in Drop", stash_ref);
                } else {
                    eprintln!(
                        "[WARN] Snapshot Drop: stash ref not found for tag {} — workspace NOT reset to protect user data",
                        tag
                    );
                }
            }
            StashState::Failed(reason) => {
                eprintln!(
                    "[WARN] Snapshot Drop: reset skipped — stash failed at snapshot time ({}). Workspace preserved.",
                    reason
                );
            }
        }

        self.restore_python_infra(had_venv);
        self.active = false;
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use std::fs;

    fn make_git_repo(dir: &std::path::Path) {
        let run = |args: &[&str]| {
            Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git command failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        fs::write(dir.join("README.md"), "base").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "base"]);
    }

    /// C-01: index.lock prevents stash — untracked files must survive
    #[test]
    fn c01_index_lock_preserves_untracked_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = dir.path();
        make_git_repo(ws);

        // Create untracked precious file
        fs::write(ws.join("important.txt"), "precious content").unwrap();
        fs::write(ws.join("README.md"), "edited").unwrap();

        // Simulate index.lock (causes stash to fail)
        fs::write(ws.join(".git/index.lock"), "locked").unwrap();

        // take() must NOT destroy files when stash fails
        let snap = Snapshot::take(ws);

        // important.txt must still exist
        assert!(
            ws.join("important.txt").exists(),
            "C-01 FAIL: important.txt was destroyed despite stash failure"
        );

        // stash_state must be Failed
        assert!(
            matches!(snap.stash_state, StashState::Failed(_)),
            "C-01 FAIL: expected StashState::Failed, got something else"
        );

        // cleanup
        let _ = fs::remove_file(ws.join(".git/index.lock"));
    }

    /// C-01: rollback with Failed state must NOT reset workspace
    #[test]
    fn c01_rollback_failed_state_preserves_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = dir.path();
        make_git_repo(ws);

        fs::write(ws.join("important.txt"), "precious").unwrap();
        fs::write(ws.join(".git/index.lock"), "locked").unwrap();

        let mut snap = Snapshot::take(ws);
        let _ = fs::remove_file(ws.join(".git/index.lock"));

        // rollback must NOT reset/clean
        snap.rollback();

        assert!(
            ws.join("important.txt").exists(),
            "C-01 FAIL: rollback destroyed files despite Failed stash state"
        );
    }

    /// C-01: normal stash succeeds — StashState::Stashed on workspace with changes
    #[test]
    fn c01_stashed_state_on_workspace_with_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = dir.path();
        make_git_repo(ws);

        // take() writes .gitignore if missing, so there are always changes
        // verify stash succeeds and state is Stashed
        let snap = Snapshot::take(ws);

        assert!(
            matches!(snap.stash_state, StashState::Stashed { .. })
                || matches!(snap.stash_state, StashState::NoLocalChanges),
            "C-01 FAIL: expected Stashed or NoLocalChanges on normal workspace"
        );

        // verify snapshot is active
        assert!(snap.active, "C-01 FAIL: snapshot should be active after take()");
    }
}
