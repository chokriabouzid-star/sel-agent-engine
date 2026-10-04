//! W2 regression (Task 2) — `mutation_check` must honour nested Cargo crates.
//!
//! RED on f0ea08e: the `"rs"` arm runs `cargo test --quiet` from
//! `self.workspace` with NO `--manifest-path`, so a crate created via
//! `cargo new add_lib --lib` inside the workspace fails with
//! "could not find `Cargo.toml`" and every mutation is misclassified
//! (Uncompilable), never actually executed.
//!
//! Spec under test:
//! 1. Upward walk from the SOURCE FILE (not cwd) to the nearest Cargo.toml,
//!    stopping at the workspace boundary; pass `--manifest-path`.
//! 2. No manifest found up to the workspace root =>
//!    MutationResult::Skipped("no Cargo manifest found") — EXACT string.

use sel_agent::executor::mutation::MutationResult;
use sel_agent::executor::SafeExecutor;
use std::path::{Path, PathBuf};

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt")
        .block_on(f)
}

fn fresh_ws(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("sel_w2_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("ws dir");
    p
}

/// Exact bug shape from the evaluation report: NO Cargo.toml at the workspace
/// root; the agent ran `cargo new add_lib --lib` in a subfolder.
fn write_nested_add_crate(ws: &Path, test_line: &str) {
    std::fs::create_dir_all(ws.join("add_lib/src")).expect("add_lib/src");
    std::fs::write(
        ws.join("add_lib/Cargo.toml"),
        "[package]\nname = \"add_lib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("nested Cargo.toml");
    std::fs::write(
        ws.join("add_lib/src/lib.rs"),
        format!(
            "pub fn add(a: i32, b: i32) -> i32 {{ a + b }}\n\n#[cfg(test)]\nmod t {{\n    #[test]\n    fn check_add() {{ {} }}\n}}\n",
            test_line
        ),
    )
    .expect("nested lib.rs");
}

#[test]
fn nested_crate_killed_mutation_is_strong() {
    let ws = fresh_ws("strong");
    // add(2,3)==5: the `a + b -> a - b` mutant yields -1 => the test KILLS it.
    write_nested_add_crate(&ws, "assert_eq!(super::add(2, 3), 5);");
    let exec = SafeExecutor::new(ws, 120);
    let res = block_on(exec.mutation_check("add_lib/src/lib.rs"));
    assert_eq!(
        res,
        MutationResult::Strong,
        "nested crate: a killed mutant must be Strong — requires --manifest-path"
    );
}

#[test]
fn nested_crate_surviving_mutation_is_weak() {
    let ws = fresh_ws("weak");
    // add(0,0)==0: the `a + b -> a - b` mutant still returns 0 => it SURVIVES.
    // Guards against false-green: proves tests really RUN in the nested crate,
    // instead of a root-level cargo failure being miscounted as a "kill".
    write_nested_add_crate(&ws, "assert_eq!(super::add(0, 0), 0);");
    let exec = SafeExecutor::new(ws, 120);
    let res = block_on(exec.mutation_check("add_lib/src/lib.rs"));
    assert!(
        matches!(res, MutationResult::Weak(_, _)),
        "nested crate: a surviving mutant must be Weak, got {:?}",
        res
    );
}

#[test]
fn isolated_rs_without_manifest_is_skipped() {
    let ws = fresh_ws("nomanifest");
    // The file DOES contain a mutable pattern (`a + b`) so we get past the
    // "No mutable patterns found" early-exit — only the missing manifest
    // can produce the expected Skipped reason.
    std::fs::write(
        ws.join("lonely.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .expect("lonely.rs");
    let exec = SafeExecutor::new(ws, 60);
    let res = block_on(exec.mutation_check("lonely.rs"));
    assert_eq!(
        res,
        MutationResult::Skipped("no Cargo manifest found".into()),
        "Task 2 spec: missing manifest must return the EXACT Skipped reason"
    );
}
