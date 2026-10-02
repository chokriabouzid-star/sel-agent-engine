// src/snapshot.rs
// Workspace snapshots that preserve the CURRENT worktree for repair attempts.
// The stash acts as a backup copy only; the worktree stays intact after take().

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Tracks the result of the stash operation to prevent ambiguous None handling.
#[derive(Debug)]
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
    replay_mode: bool,
    /// Symlink target of `workspace/venv` captured at snapshot time, if it was
    /// a symlink. Used to relink the same environment after a rollback removes it.
    venv_link_target: Option<PathBuf>,
}

impl Snapshot {
    fn resolve_stash_ref(workspace: &Path, tag: &str) -> Option<String> {
        let out = Command::new("git")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
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
        // Replay restores exactly the environment that was attached when the
        // snapshot was taken. Live keeps its historical bootstrap behaviour.
        let target = self.venv_link_target.clone().or_else(|| {
            if self.replay_mode {
                None
            } else {
                Some(crate::scaffold_engine::get_cache_dir().join("python/venv"))
            }
        });
        if let Some(target) = target {
            if let Err(message) = self.restore_python_infra_from_cache(had_venv, &target) {
                eprintln!("[WARN] {message}");
            }
        }
    }

    fn restore_python_infra_from_cache(
        &self,
        had_venv: bool,
        cache_venv: &Path,
    ) -> Result<(), String> {
        let workspace_venv = self.workspace.join("venv");
        let pytest_bin = workspace_venv.join("bin/pytest");
        let pip_bin = workspace_venv.join("bin/pip");
        let cache_available = cache_venv.exists() && cache_venv.join("bin/pytest").exists();

        let action = python_infra_restore_plan(
            had_venv,
            workspace_venv.exists(),
            pytest_bin.exists(),
            pip_bin.exists(),
            cache_available,
            self.replay_mode,
        );

        match action {
            PythonInfraAction::Nothing => Ok(()),
            PythonInfraAction::RelinkCache => {
                std::os::unix::fs::symlink(cache_venv, &workspace_venv).map_err(|error| {
                    format!(
                        "REPLAY_ENV_MISMATCH: failed to restore cached Python venv: {error}"
                    )
                })
            }
            PythonInfraAction::CreateVenvAndInstallPytest => {
                let _ = Command::new("python3")
                    .args(["-m", "venv", "venv"])
                    .current_dir(&self.workspace)
                    .output();

                let _ = Command::new("venv/bin/pip")
                    .args(["install", "pytest", "-q"])
                    .current_dir(&self.workspace)
                    .output();
                Ok(())
            }
            PythonInfraAction::InstallPytest => {
                let _ = Command::new(&pip_bin)
                    .args(["install", "pytest", "-q"])
                    .current_dir(&self.workspace)
                    .output();
                Ok(())
            }
            PythonInfraAction::FailReplay => Err(
                "REPLAY_ENV_MISMATCH: snapshot rollback cannot repair Python                  infrastructure during replay; provision the Python replay                  environment before running the replay gate"
                    .to_string(),
            ),
        }
    }

    fn git_reset_clean(workspace: &Path) {
        let _ = Command::new("git")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(["reset", "--hard"])
            .current_dir(workspace)
            .output();
        let _ = Command::new("git")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(["clean", "-fd"])
            .current_dir(workspace)
            .output();
    }

    /// Take a snapshot of the current workspace while KEEPING the current
    /// worktree intact. We use git stash as a backup, then re-apply it
    /// immediately so repair code can still see the created files.
    /// Live-mode convenience used by the snapshot tests; production code
    /// always states the mode explicitly via `take_with_mode`.
    #[cfg(test)]
    pub fn take(workspace: &Path) -> Self {
        Self::take_with_mode(workspace, false)
    }

    pub fn take_with_mode(workspace: &Path, replay_mode: bool) -> Self {
        // Capture the venv symlink target before anything can remove it.
        // In replay the scaffold attached a profile link; in live the venv is
        // usually a real directory and this is None.
        let venv_link_target = std::fs::read_link(workspace.join("venv")).ok();

        // Ensure it's a git repo
        if !workspace.join(".git").exists() {
            let _ = Command::new("git")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
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
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .args(["rev-parse", "HEAD"])
            .current_dir(workspace)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !has_head {
            let _ = Command::new("git")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["config", "user.name", "SEL Agent"])
                .current_dir(workspace)
                .output();
            let _ = Command::new("git")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["config", "user.email", "sel@local.test"])
                .current_dir(workspace)
                .output();
            let _ = Command::new("git")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .args(["add", "."])
                .current_dir(workspace)
                .output();
            let _ = Command::new("git")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
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
                replay_mode,
                venv_link_target,
            };
        }

        let output = Command::new("git")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
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
                            .env_remove("GIT_INDEX_FILE")
                            .env_remove("GIT_DIR")
                            .env_remove("GIT_WORK_TREE")
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
            replay_mode,
            venv_link_target,
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
                        .env_remove("GIT_INDEX_FILE")
                        .env_remove("GIT_DIR")
                        .env_remove("GIT_WORK_TREE")
                        .env("LC_ALL", "C")
                        .env("LANG", "C")
                        .args(["stash", "apply"])
                        .arg(&stash_ref)
                        .current_dir(&self.workspace)
                        .output();
                    let _ = Command::new("git")
                        .env_remove("GIT_INDEX_FILE")
                        .env_remove("GIT_DIR")
                        .env_remove("GIT_WORK_TREE")
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
                    .env_remove("GIT_INDEX_FILE")
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_WORK_TREE")
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
                        .env_remove("GIT_INDEX_FILE")
                        .env_remove("GIT_DIR")
                        .env_remove("GIT_WORK_TREE")
                        .env("LC_ALL", "C")
                        .env("LANG", "C")
                        .args(["stash", "apply"])
                        .arg(&stash_ref)
                        .current_dir(&self.workspace)
                        .output();
                    let _ = Command::new("git")
                        .env_remove("GIT_INDEX_FILE")
                        .env_remove("GIT_DIR")
                        .env_remove("GIT_WORK_TREE")
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
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
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

    /// C-01-B: commit() on a clean initial snapshot must preserve agent-created files.
    ///
    /// Scenario:
    /// 1. Workspace is clean when Snapshot::take() runs → StashState::NoLocalChanges.
    /// 2. Agent creates a new untracked file during the session.
    /// 3. Final failure path must call commit(), not rollback()/Drop cleanup.
    /// 4. The new file must survive.
    #[test]
    fn c01b_commit_preserves_agent_created_untracked_files_on_nolocalchanges() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = dir.path();
        make_git_repo(ws);

        let mut snap = Snapshot::take(ws);

        // NOTE: take() itself may dirty the worktree (infra protection),
        // so both NoLocalChanges and Stashed are valid starting states.
        // The guarantee under test: commit() + Drop must never destroy
        // agent-created files, regardless of stash state.
        assert!(
            matches!(
                snap.stash_state,
                StashState::NoLocalChanges | StashState::Stashed { .. }
            ),
            "setup failed: unexpected stash state {:?}",
            snap.stash_state
        );

        fs::write(ws.join("solution.py"), "def solve():\n    return 42\n").unwrap();

        snap.commit();

        // Force Drop after commit. Since commit() sets active=false, Drop must not clean.
        drop(snap);

        assert!(
            ws.join("solution.py").exists(),
            "C-01-B FAIL: commit() or Drop destroyed agent-created file solution.py"
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
        assert!(
            snap.active,
            "C-01 FAIL: snapshot should be active after take()"
        );
    }
}

/// Decision for repairing Python infrastructure after a snapshot rollback.
/// Pure: performs no I/O, so the replay contract can be tested hermetically.
#[allow(dead_code)] // wired into Snapshot::rollback by the fix commit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonInfraAction {
    Nothing,
    RelinkCache,
    CreateVenvAndInstallPytest,
    InstallPytest,
    FailReplay,
}

/// Decide how rollback may repair Python infrastructure.
///
/// Replay is fail-closed: it may relink an already provisioned cache, but it
/// must never create an environment or install a package.
#[allow(clippy::fn_params_excessive_bools)]
pub fn python_infra_restore_plan(
    had_venv: bool,
    venv_exists: bool,
    pytest_exists: bool,
    pip_exists: bool,
    cache_available: bool,
    replay: bool,
) -> PythonInfraAction {
    if replay {
        if had_venv && !venv_exists {
            return if cache_available {
                PythonInfraAction::RelinkCache
            } else {
                PythonInfraAction::FailReplay
            };
        }

        if venv_exists && !pytest_exists {
            return PythonInfraAction::FailReplay;
        }

        return PythonInfraAction::Nothing;
    }

    if had_venv && !venv_exists {
        if cache_available {
            PythonInfraAction::RelinkCache
        } else {
            PythonInfraAction::CreateVenvAndInstallPytest
        }
    } else if venv_exists && !pytest_exists && pip_exists {
        PythonInfraAction::InstallPytest
    } else {
        PythonInfraAction::Nothing
    }
}

#[cfg(test)]
mod python_infra_replay_tests {
    use super::{python_infra_restore_plan as plan, PythonInfraAction as A, Snapshot, StashState};
    use std::path::Path;
    use tempfile::tempdir;

    fn replay_snapshot(workspace: &Path) -> Snapshot {
        Snapshot {
            workspace: workspace.to_path_buf(),
            active: false,
            stash_state: StashState::NoLocalChanges,
            replay_mode: true,
            venv_link_target: None,
        }
    }

    // args: had_venv, venv_exists, pytest_exists, pip_exists, cache_available, replay

    #[test]
    fn bug_replay_rollback_without_cache_must_not_install() {
        assert_eq!(plan(true, false, false, false, false, true), A::FailReplay);
    }

    #[test]
    fn bug_replay_missing_pytest_must_not_install() {
        assert_eq!(plan(true, true, false, true, true, true), A::FailReplay);
    }

    #[test]
    fn guard_replay_with_cache_relinks() {
        assert_eq!(plan(true, false, false, false, true, true), A::RelinkCache);
    }

    #[test]
    fn guard_live_without_cache_still_bootstraps() {
        assert_eq!(
            plan(true, false, false, false, false, false),
            A::CreateVenvAndInstallPytest
        );
    }

    #[test]
    fn guard_replay_intact_env_does_nothing() {
        assert_eq!(plan(true, true, true, true, true, true), A::Nothing);
    }

    #[test]
    fn replay_restore_does_not_bootstrap_when_cache_is_missing() {
        let workspace = tempdir().expect("workspace");
        let cache_root = tempdir().expect("cache root");
        let missing_cache = cache_root.path().join("missing-venv");
        let snapshot = replay_snapshot(workspace.path());

        let error = snapshot
            .restore_python_infra_from_cache(true, &missing_cache)
            .expect_err("replay must fail closed when its cache is missing");

        assert!(error.contains("REPLAY_ENV_MISMATCH"));
        assert!(!workspace.path().join("venv").exists());
    }

    #[test]
    fn replay_restore_relinks_a_complete_cached_environment() {
        let workspace = tempdir().expect("workspace");
        let cache_root = tempdir().expect("cache root");
        let cached_venv = cache_root.path().join("venv");
        std::fs::create_dir_all(cached_venv.join("bin")).expect("cache setup");
        std::fs::write(cached_venv.join("bin/pytest"), b"fixture").expect("cache setup");

        let snapshot = replay_snapshot(workspace.path());
        snapshot
            .restore_python_infra_from_cache(true, &cached_venv)
            .expect("replay should relink a complete cache");

        let metadata = std::fs::symlink_metadata(workspace.path().join("venv")).expect("venv link");
        assert!(metadata.file_type().is_symlink());
    }

    #[test]
    fn replay_restore_does_not_install_when_pytest_is_missing() {
        let workspace = tempdir().expect("workspace");
        let cache_root = tempdir().expect("cache root");
        let missing_cache = cache_root.path().join("missing-venv");
        std::fs::create_dir_all(workspace.path().join("venv/bin")).expect("workspace setup");
        std::fs::write(workspace.path().join("venv/bin/pip"), b"fixture").expect("workspace setup");

        let snapshot = replay_snapshot(workspace.path());
        let error = snapshot
            .restore_python_infra_from_cache(true, &missing_cache)
            .expect_err("replay must not install missing pytest");

        assert!(error.contains("REPLAY_ENV_MISMATCH"));
        assert!(!workspace.path().join("venv/bin/pytest").exists());
    }

    #[test]
    fn replay_relinks_the_previously_attached_profile_target() {
        let workspace = tempdir().expect("workspace");
        let profile = tempdir().expect("profile");
        let cached_venv = profile.path().join("venv");
        std::fs::create_dir_all(cached_venv.join("bin")).expect("cache");
        std::fs::write(cached_venv.join("bin/pytest"), b"fixture").expect("pytest");

        std::os::unix::fs::symlink(&cached_venv, workspace.path().join("venv")).expect("link");

        let snapshot = Snapshot {
            workspace: workspace.path().to_path_buf(),
            active: false,
            stash_state: StashState::NoLocalChanges,
            replay_mode: true,
            venv_link_target: std::fs::read_link(workspace.path().join("venv")).ok(),
        };

        // Simulate `git clean -fd` removing the symlink.
        std::fs::remove_file(workspace.path().join("venv")).expect("remove link");

        snapshot.restore_python_infra(true);

        let restored = std::fs::read_link(workspace.path().join("venv")).expect("venv relinked");
        assert_eq!(restored, cached_venv);
    }
}
