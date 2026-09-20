//! Phase 3 (roadmap 2026-09-16, §6) — Oracle negative regressions.
//! Status: expected to FAIL on HEAD a752295 for the *_bug_* tests
//! (proving goal_requires_go_race substring gaps + no nested-manifest discovery).
//! The *_guard_* tests must already PASS (they protect P0 behavior).

use sel_agent::workspace_oracle::{goal_requires_go_race, ProjectType, WorkspaceOracle};
use std::path::PathBuf;

fn fresh_ws(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("sel_p3_oracle_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("ws dir");
    p
}

// ---- BUG 1: negated race goal must NOT force -race ----
#[test]
fn bug_race_negation_detector_phrase() {
    assert!(
        !goal_requires_go_race("Fix bug. DO NOT enable the race detector. Run go test."),
        "explicit negation of the race detector must not force -race"
    );
}

#[test]
fn bug_race_negation_dash_race_with_pass() {
    assert!(
        !goal_requires_go_race("Do not add -race; all tests must pass."),
        "'-race' inside a negation + 'pass' must not force -race"
    );
}

// ---- BUG 2: nested Rust manifest must be discovered ----
#[test]
fn bug_nested_rust_manifest_detected() {
    let ws = fresh_ws("rust_sub");
    std::fs::create_dir_all(ws.join("mylib")).unwrap();
    std::fs::write(
        ws.join("mylib/Cargo.toml"),
        "[package]\nname=\"mylib\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(ws.join("mylib/lib.rs"), "pub fn x() {}\n").unwrap();

    let t = WorkspaceOracle::new(ws).current_type();
    assert_eq!(
        t,
        ProjectType::Rust,
        "nested mylib/Cargo.toml must be detected as Rust, got {:?}",
        t
    );
}

// ---- GUARDS: these MUST already pass (do not break P0) ----
#[test]
fn guard_positive_race_goal_still_true() {
    assert!(goal_requires_go_race("go test -race ./... must pass"));
    assert!(goal_requires_go_race("ensure -race ./... passes"));
}

#[test]
fn guard_plain_go_goal_still_false() {
    assert!(!goal_requires_go_race("go test ./... must pass"));
    assert!(!goal_requires_go_race(""));
}

#[test]
fn guard_root_rust_still_detected() {
    let ws = fresh_ws("rust_root");
    std::fs::write(ws.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
    assert_eq!(WorkspaceOracle::new(ws).current_type(), ProjectType::Rust);
}
