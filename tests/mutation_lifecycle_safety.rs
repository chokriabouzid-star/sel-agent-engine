//! W4 part 1 regression — `mutation_check` must not leak mutant test
//! processes and must honour the executor timeout (capped at 30s).
//!
//! RED on current HEAD: the `mutation_check` loop calls
//! `tokio::time::timeout(30s, Command::output())`. On timeout the future
//! is dropped: tokio kills the DIRECT child only, grandchildren survive,
//! and `self.timeout_secs` is ignored entirely (hardcoded 30s).
//!
//! Spec under test:
//! 1. A hanging mutant run must leave NO live descendants after return.
//! 2. The per-mutant deadline is `min(self.timeout_secs, 30)`.

#![cfg(unix)]

use sel_agent::executor::{MutationResult, SafeExecutor};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt")
        .block_on(f)
}

fn fresh_ws(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("sel_w4_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("ws dir");
    p
}

fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn mutation_timeout_kills_grandchild_and_honours_executor_timeout() {
    let ws = fresh_ws("mut_orphan");
    let pid_file = ws.join("grandchild.pid");

    // Source with EXACTLY ONE mutable pattern (" + " only; no `a + b`,
    // no `return 0/1/42`) so the loop runs a single mutant.
    std::fs::write(ws.join("calc.py"), "def add(x, y):\n    return x + y\n")
        .expect("write calc.py");

    // Fake pytest: mutation_check prefers `venv/bin/pytest` when present.
    // It spawns a grandchild (sleep 300), records its PID, then hangs.
    std::fs::create_dir_all(ws.join("venv/bin")).expect("venv/bin");
    let fake_pytest = ws.join("venv/bin/pytest");
    std::fs::write(
        &fake_pytest,
        format!(
            "#!/bin/sh\nsleep 300 &\necho $! > '{}'\nsleep 300\n",
            pid_file.display()
        ),
    )
    .expect("write fake pytest");
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&fake_pytest).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake_pytest, perms).unwrap();

    // Executor timeout = 1s. Spec: per-mutant deadline = min(1, 30) = 1s.
    let exec = SafeExecutor::new(ws.clone(), 1);
    let started = Instant::now();
    let res = block_on(exec.mutation_check("calc.py"));
    let elapsed = started.elapsed();

    std::thread::sleep(Duration::from_millis(50));

    // Missing/invalid PID file must FAIL the test, never skip the check.
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("fixture did not record the grandchild PID")
        .trim()
        .parse()
        .expect("fixture recorded an invalid grandchild PID");

    let alive = is_process_alive(pid);
    if alive {
        // Clean up so the RED run itself does not leak.
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }

    // A hang is neither a kill nor a survivor.
    assert!(
        !matches!(res, MutationResult::Weak(_, _)),
        "a hanging mutant must not be classified Weak, got {:?}",
        res
    );
    assert!(
        !alive,
        "grandchild PID {} is still alive after mutation_check returned",
        pid
    );
    assert!(
        elapsed < Duration::from_secs(15),
        "mutation_check must honour min(timeout_secs=1, 30); took {:?}",
        elapsed
    );
}
