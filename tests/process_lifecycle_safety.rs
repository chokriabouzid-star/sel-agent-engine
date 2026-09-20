//! Phase 2 (roadmap 2026-09-16, §5) — Process Lifecycle & Cleanup Regressions.
//!
//! Contract under test: When a test or command times out, the underlying OS
//! process and all its children MUST be terminated BEFORE the call returns,
//! ensuring no orphan processes continue running during Repairing.
//!
//! Status: Expected to FAIL on HEAD 8c9ad86 (Proving the orphan process bug).

use sel_agent::executor::SafeExecutor;
use std::path::PathBuf;
use std::time::Duration;

fn fresh_ws(tag: &str) -> PathBuf {
    let ws = std::env::temp_dir().join(format!("sel_p2_lifecycle_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).expect("create temp workspace");
    ws
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(f)
}

#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    let out = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output();
    match out {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Test 1: An async shell command that times out must NOT leave an orphaned process running.
#[test]
#[cfg(unix)]
fn lifecycle_1_timed_out_shell_process_must_be_killed() {
    let ws = fresh_ws("shell_orphan");
    let pid_file = ws.join("child.pid");

    // Script records its own PID and sleeps for 30 seconds
    let script_content = format!("#!/bin/sh\necho $$ > '{}'\nsleep 30\n", pid_file.display());
    let script_path = ws.join("hang.sh");
    std::fs::write(&script_path, script_content).expect("write script");

    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();

    // Executor with 1-second timeout
    let exec = SafeExecutor::new(ws.clone(), 1);

    let cmd = sel_agent::protocol::Cmd::Run {
        command: format!("./{}", script_path.file_name().unwrap().to_str().unwrap()),
    };

    let _ = block_on(exec.run(&cmd));

    // Allow a tiny grace window (50ms) for OS cleanup
    std::thread::sleep(Duration::from_millis(50));

    if pid_file.exists() {
        let pid_str = std::fs::read_to_string(&pid_file).unwrap_or_default();
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            let alive = is_process_alive(pid);
            // If the process is still alive, clean it up manually so we don't leak test processes
            if alive {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
            }
            assert!(
                !alive,
                "Process with PID {} is still running after timeout was triggered!",
                pid
            );
        }
    }
}

/// Test 2: Node.js test runner timeout must kill the test runner process.
#[test]
#[cfg(unix)]
fn lifecycle_2_timed_out_node_test_runner_must_be_killed() {
    let ws = fresh_ws("node_orphan");
    let pid_file = ws.join("node.pid");

    let script = format!(
        "const fs = require('fs');\nfs.writeFileSync('{}', String(process.pid));\nsetInterval(() => {{}}, 1000);\n",
        pid_file.display()
    );
    std::fs::write(ws.join("hang.js"), script).expect("write hang.js");

    let exec = SafeExecutor::new(ws.clone(), 1);
    let r = block_on(exec.run_tests("node hang.js"));

    // Timeout must return Ok(ExecResult::fail)
    assert!(r.is_ok(), "run_tests should return Ok on timeout");

    std::thread::sleep(Duration::from_millis(50));

    if pid_file.exists() {
        let pid_str = std::fs::read_to_string(&pid_file).unwrap_or_default();
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            let alive = is_process_alive(pid);
            if alive {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
            }
            assert!(
                !alive,
                "Node runner with PID {} is still running after runner timeout!",
                pid
            );
        }
    }
}
