//! W4 part 2 regression — cancelling `mutation_check` mid-run must restore
//! the user's original source file.
//!
//! RED on current HEAD: restoration is four sequential
//! `std::fs::write(&source_path, &original)` statements placed AFTER the
//! only `.await` in the loop (the `output_with_timeout` call). Dropping
//! the future at that await skips every one of them, so the file stays
//! MUTATED on disk.
//!
//! Spec under test: on ANY exit path — normal return, early return,
//! panic, or future cancellation — the source file ends with its
//! original content.

#![cfg(unix)]

use sel_agent::executor::SafeExecutor;
use std::path::PathBuf;
use std::time::Duration;

const ORIGINAL: &str = "def add(x, y):\n    return x + y\n";

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt")
        .block_on(f)
}

fn fresh_ws(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("sel_w4raii_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("ws dir");
    p
}

#[test]
fn cancelled_mutation_check_restores_original_source() {
    let ws = fresh_ws("cancel_restore");

    // Source with EXACTLY ONE mutable pattern (" + ") so the loop has a
    // single mutant and therefore a single cancellation window.
    std::fs::write(ws.join("calc.py"), ORIGINAL).expect("write calc.py");

    // Fake pytest: mutation_check prefers `venv/bin/pytest` when present.
    // It records that it started, then `exec` replaces the shell with
    // `sleep` so the direct child IS the sleeper — kill_on_drop cleans it
    // fully, no grandchild can leak from this test.
    let started = ws.join("mutant.started");
    std::fs::create_dir_all(ws.join("venv/bin")).expect("venv/bin");
    let fake_pytest = ws.join("venv/bin/pytest");
    std::fs::write(
        &fake_pytest,
        format!("#!/bin/sh\ntouch '{}'\nexec sleep 300\n", started.display()),
    )
    .expect("write fake pytest");
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&fake_pytest).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake_pytest, perms).unwrap();

    let ws_task = ws.clone();
    block_on(async move {
        // timeout_secs=10: the internal deadline must NOT fire first;
        // cancellation must come from US, not from the executor.
        let exec = SafeExecutor::new(ws_task.clone(), 10);
        // Box::pin (NOT tokio::pin!): dropping the pinned box destroys the
        // future IMMEDIATELY. tokio::pin! would defer the real drop to
        // scope end — after our assertions — corrupting both RED and GREEN.
        let mut fut = Box::pin(exec.mutation_check("calc.py"));

        // Drive the future until the mutant runner is provably running.
        let mut ticks = 0u32;
        loop {
            tokio::select! {
                res = &mut fut => {
                    panic!(
                        "fixture broken: mutation_check finished before cancellation: {:?}",
                        res
                    );
                }
                _ = tokio::time::sleep(Duration::from_millis(25)) => {
                    if started.exists() {
                        break;
                    }
                    ticks += 1;
                    assert!(ticks < 200, "fixture broken: mutant runner never started");
                }
            }
        }

        // Precondition (fixture sanity, NOT the contract): the file on disk
        // must be the MUTANT right now, or this test proves nothing.
        let during = std::fs::read_to_string(ws_task.join("calc.py")).expect("read during");
        assert_ne!(
            during, ORIGINAL,
            "fixture broken: file was not mutated while the mutant was running"
        );

        // THE CANCELLATION: drop the future at its only .await point.
        drop(fut);

        // THE CONTRACT: original content must be back on disk.
        let after = std::fs::read_to_string(ws_task.join("calc.py")).expect("read after");
        assert_eq!(
            after, ORIGINAL,
            "cancelled mutation_check left the user's source MUTATED on disk"
        );
    });
}
