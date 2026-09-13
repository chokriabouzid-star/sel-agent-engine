// src/snapshot.rs
// Workspace snapshots that preserve the CURRENT worktree for repair attempts.
// The stash acts as a backup copy only; the worktree stays intact after take().

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Snapshot {
    workspace: PathBuf,
    active: bool,
    stash_tag: Option<String>,
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

        // Ensure pytest exists if venv exists
        let pytest_bin = self.workspace.join("venv/bin/pytest");
        let pip_bin = self.workspace.join("venv/bin/pip");
        if self.workspace.join("venv").exists() && !pytest_bin.exists() && pip_bin.exists() {
            let _ = Command::new(&pip_bin)
                .args(["install", "pytest", "-q"])
                .current_dir(&self.workspace)
                .output();
        }
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

        // FIX C-01: check status.success() — exit=1 means stash failed
        let output = Command::new("git")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(["stash", "push", "--include-untracked", "-m"])
            .arg(&tag)
            .current_dir(workspace)
            .output();

        let has_stashed = if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            out.status.success() && !stdout.contains("No local changes to save")
        } else {
            false
        };

        let stash_tag = if has_stashed {
            if let Some(stash_ref) = Self::resolve_stash_ref(workspace, &tag) {
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
                        eprintln!("[TRACE] Snapshot: failed to re-apply {}: {}", stash_ref, e);
                    }
                }
                Some(tag)
            } else {
                eprintln!(
                    "[WARN] Snapshot: stash exit=0 but ref not found for tag {} — treating as no stash",
                    tag
                );
                None
            }
        } else {
            None
        };

        Self {
            workspace: workspace.to_path_buf(),
            active: true,
            stash_tag,
        }
    }

    /// Rollback workspace to the pre-attempt state stored in this snapshot.
    pub fn rollback(&mut self) {
        if !self.active {
            return;
        }

        let had_venv = self.workspace.join("venv").exists();

        if let Some(tag) = self.stash_tag.as_deref() {
            if let Some(stash_ref) = Self::resolve_stash_ref(&self.workspace, tag) {
                // FIX C-01: reset/clean ONLY when we have a verified stash to restore from
                let _ = Command::new("git")
                    .env("LC_ALL", "C")
                    .env("LANG", "C")
                    .args(["reset", "--hard"])
                    .current_dir(&self.workspace)
                    .output();

                let _ = Command::new("git")
                    .env("LC_ALL", "C")
                    .env("LANG", "C")
                    .args(["clean", "-fd"])
                    .current_dir(&self.workspace)
                    .output();

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
                // stash_tag set but ref missing — do NOT touch files to protect user data
                eprintln!(
                    "[WARN] Snapshot: stash ref not found for tag {} — workspace NOT reset to protect user data",
                    tag
                );
            }
        } else {
            // No stash was created (no changes existed) — safe to reset
            let _ = Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["reset", "--hard"])
                .current_dir(&self.workspace)
                .output();

            let _ = Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["clean", "-fd"])
                .current_dir(&self.workspace)
                .output();

            println!("    Snapshot: reset workspace (no stash needed)");
        }

        self.restore_python_infra(had_venv);
        self.active = false;
    }

    /// Accept the current changes and discard the backup stash.
    pub fn commit(&mut self) {
        if !self.active {
            return;
        }

        if let Some(tag) = self.stash_tag.as_deref() {
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

        if let Some(tag) = self.stash_tag.as_deref() {
            if let Some(stash_ref) = Self::resolve_stash_ref(&self.workspace, tag) {
                // FIX C-01: reset/clean ONLY when we have a verified stash to restore from
                let _ = Command::new("git")
                    .env("LC_ALL", "C")
                    .env("LANG", "C")
                    .args(["reset", "--hard"])
                    .current_dir(&self.workspace)
                    .output();

                let _ = Command::new("git")
                    .env("LC_ALL", "C")
                    .env("LANG", "C")
                    .args(["clean", "-fd"])
                    .current_dir(&self.workspace)
                    .output();

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
                // stash_tag set but ref missing — do NOT touch files to protect user data
                eprintln!(
                    "[WARN] Snapshot: stash ref not found for tag {} in Drop — workspace NOT reset to protect user data",
                    tag
                );
            }
        } else {
            // No stash was created — safe to reset
            let _ = Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["reset", "--hard"])
                .current_dir(&self.workspace)
                .output();

            let _ = Command::new("git")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["clean", "-fd"])
                .current_dir(&self.workspace)
                .output();
        }

        self.restore_python_infra(had_venv);
        self.active = false;
    }
}
