// src/snapshot.rs — v7.4: Workspace Snapshots
// Uses Git-based snapshot strategy (git stash) as requested for atomic rollbacks

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Snapshot {
    workspace: PathBuf,
    active: bool,
    has_stashed: bool,
}

impl Snapshot {
    /// Take a snapshot of the workspace using git stash
    pub fn take(workspace: &Path) -> Self {
        // Ensure it's a git repo
        if !workspace.join(".git").exists() {
            let _ = Command::new("git")
                .arg("init")
                .current_dir(workspace)
                .output();
        }

        // Add all files so git tracks them (otherwise untracked files aren't stashed without -u)
        let _ = Command::new("git")
            .arg("add")
            .arg(".")
            .current_dir(workspace)
            .output();

        // Perform stash
        let output = Command::new("git")
            .args(&["stash", "push", "--include-untracked", "-m", "sel_agent_snapshot"])
            .current_dir(workspace)
            .output();

        let has_stashed = if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            !stdout.contains("No local changes to save")
        } else {
            false
        };

        if has_stashed {
            eprintln!("[TRACE] Snapshot: git stash created successfully");
        }

        Self {
            workspace: workspace.to_path_buf(),
            active: true,
            has_stashed,
        }
    }

    /// Rollback workspace to pre-change state
    pub fn rollback(&mut self) {
        if !self.active { return; }
        
        // Discard any current changes made during the failed step
        let _ = Command::new("git")
            .args(&["reset", "--hard"])
            .current_dir(&self.workspace)
            .output();
            
        let _ = Command::new("git")
            .args(&["clean", "-fd"])
            .current_dir(&self.workspace)
            .output();

        if self.has_stashed {
            // Restore the stash
            let _ = Command::new("git")
                .args(&["stash", "pop"])
                .current_dir(&self.workspace)
                .output();
            println!("   ⏪ Snapshot: rolled back via git stash pop");
        } else {
            println!("   ⏪ Snapshot: reset workspace (no stash needed)");
        }
        self.active = false;
    }

    /// Commit — accept the changes, discard backup
    pub fn commit(&mut self) {
        if !self.active { return; }
        
        if self.has_stashed {
            // Drop the stash since we're keeping the new changes
            let _ = Command::new("git")
                .args(&["stash", "drop"])
                .current_dir(&self.workspace)
                .output();
            eprintln!("[TRACE] Snapshot: git stash dropped (changes accepted)");
        }
        self.active = false;
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        if self.active && self.has_stashed {
            // Abnormal exit, try to rollback
            let _ = Command::new("git")
                .args(&["reset", "--hard"])
                .current_dir(&self.workspace)
                .output();
            let _ = Command::new("git")
                .args(&["clean", "-fd"])
                .current_dir(&self.workspace)
                .output();
            let _ = Command::new("git")
                .args(&["stash", "pop"])
                .current_dir(&self.workspace)
                .output();
        }
    }
}
