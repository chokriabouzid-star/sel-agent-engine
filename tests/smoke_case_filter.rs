//! W12: `SMOKE_CASES` filter for scripts/sel_smoke_test.sh.
//!
//! Fully offline: a stub binary records the goal of every `run` invocation and
//! prints SEL_SUCCESS. No network, no provider, no trajectories are touched.
#![cfg(unix)]

use regex::Regex;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

const IDS: [&str; 12] = [
    "smoke_py_binary_search",
    "smoke_py_temperature",
    "smoke_go_worker_pool",
    "smoke_go_generics",
    "smoke_ts_utils",
    "smoke_ts_retry",
    "smoke_rust_csv",
    "smoke_rust_fib",
    "smoke_ts_fastapi_client",
    "smoke_go_concurrent",
    "smoke_py_dataclass",
    "smoke_rust_trait_impl",
];

const GOAL_PREFIXES: [(&str, &str); 12] = [
    ("smoke_py_binary_search", "Fix this broken binary search"),
    ("smoke_py_temperature", "Create a Temperature class"),
    ("smoke_go_worker_pool", "Create a simple Go worker pool"),
    ("smoke_go_generics", "Create a generic stack in Go"),
    ("smoke_ts_utils", "Create TypeScript utility functions"),
    ("smoke_ts_retry", "Create a TypeScript async retry"),
    ("smoke_rust_csv", "Create a simple CSV line parser"),
    ("smoke_rust_fib", "Create a Fibonacci calculator"),
    ("smoke_ts_fastapi_client", "Write a TypeScript client"),
    ("smoke_go_concurrent", "Write a Go struct SafeCounter"),
    ("smoke_py_dataclass", "Write a Python User dataclass"),
    ("smoke_rust_trait_impl", "Implement the std::fmt::Display"),
];

struct SmokeRun {
    success: bool,
    stdout: String,
    stderr: String,
    goals: Vec<String>,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run_smoke(cases: Option<&str>) -> SmokeRun {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("calls.log");
    let stub = dir.path().join("stub-sel-agent");
    // Invocation shape: stub run --workspace WS --goal GOAL ... => goal is $5.
    let body = format!(
        "#!/usr/bin/env bash\nprintf 'CALL %s\\n' \"$5\" >> '{}'\necho SEL_SUCCESS\n",
        log.display()
    );
    fs::write(&stub, body).expect("write stub");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("chmod stub");

    let mut cmd = Command::new("bash");
    cmd.current_dir(repo_root())
        .arg("scripts/sel_smoke_test.sh")
        .arg(&stub)
        .arg("--replay")
        .env_remove("SMOKE_CASES");
    if let Some(value) = cases {
        cmd.env("SMOKE_CASES", value);
    }
    let out = cmd.output().expect("run smoke script");

    let goals = fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("CALL "))
        .map(str::to_string)
        .collect();

    SmokeRun {
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        goals,
    }
}

fn combined(run: &SmokeRun) -> String {
    format!("{}\n{}", run.stdout, run.stderr)
}

fn strip_ansi(text: &str) -> String {
    Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]")
        .unwrap()
        .replace_all(text, "")
        .into_owned()
}

#[test]
fn guard_default_runs_all_twelve_cases() {
    let run = run_smoke(None);
    assert!(run.success, "default run must succeed with a passing stub");
    assert_eq!(run.goals.len(), 12, "default must run all 12 cases");
    let text = strip_ansi(&combined(&run));
    assert!(
        text.contains("12 / 12"),
        "default summary must keep `12 / 12`"
    );
    assert!(
        !text.contains("PARTIAL RUN"),
        "default run must not be labelled partial"
    );

    // All 12 IDs selected explicitly must behave as a full run, not partial.
    let all_ids = IDS.join(",");
    let explicit_all = run_smoke(Some(&all_ids));
    assert!(
        explicit_all.success,
        "all-12 explicit selection must succeed"
    );
    assert_eq!(explicit_all.goals.len(), 12, "all-12 must run 12 cases");
    let explicit_text = strip_ansi(&combined(&explicit_all));
    assert!(
        !explicit_text.contains("PARTIAL RUN"),
        "selecting all 12 IDs explicitly must not be labelled partial"
    );
    assert!(
        explicit_text.contains("12 / 12"),
        "all-12 explicit must show full summary with 12 / 12"
    );
}

#[test]
fn guard_run_case_ids_match_expected_list() {
    let src = fs::read_to_string(repo_root().join("scripts/sel_smoke_test.sh")).unwrap();
    let ids: Vec<String> = src
        .lines()
        .filter_map(|l| l.strip_prefix("run_case \""))
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_string)
        .collect();
    assert_eq!(ids, IDS);
}

#[test]
fn single_case_runs_exactly_one_invocation() {
    let run = run_smoke(Some("smoke_py_binary_search"));
    assert!(run.success, "partial run with a passing stub must succeed");
    assert_eq!(
        run.goals.len(),
        1,
        "SMOKE_CASES=smoke_py_binary_search must run exactly one case, got {}",
        run.goals.len()
    );
    assert!(
        run.goals[0].starts_with("Fix this broken binary search"),
        "wrong case ran: {:?}",
        run.goals[0]
    );
}

#[test]
fn each_id_selects_exactly_itself() {
    for id in IDS {
        let run = run_smoke(Some(id));
        assert_eq!(
            run.goals.len(),
            1,
            "SMOKE_CASES={id} must run exactly one case, got {}",
            run.goals.len()
        );
        let expected_prefix = GOAL_PREFIXES
            .iter()
            .find(|(name, _)| *name == id)
            .unwrap_or_else(|| panic!("no prefix for {id}"))
            .1;
        assert!(
            run.goals[0].starts_with(expected_prefix),
            "SMOKE_CASES={id} ran wrong goal: {:?} (expected prefix: {expected_prefix:?})",
            &run.goals[0][..run.goals[0].len().min(60)]
        );
    }
}

#[test]
fn two_cases_run_in_script_order() {
    let run = run_smoke(Some("smoke_go_generics,smoke_py_binary_search"));
    assert_eq!(run.goals.len(), 2, "two selected cases must run twice");
    assert!(run.goals[0].starts_with("Fix this broken binary search"));
    assert!(run.goals[1].starts_with("Create a generic stack in Go"));
}

#[test]
fn unknown_case_is_rejected_before_any_run() {
    let run = run_smoke(Some("smoke_does_not_exist"));
    assert!(!run.success, "unknown case id must exit non-zero");
    assert!(
        run.goals.is_empty(),
        "no case may run when validation fails, ran {}",
        run.goals.len()
    );
    let text = combined(&run);
    assert!(
        text.contains("smoke_does_not_exist"),
        "must name the bad id"
    );
    assert!(
        text.contains("smoke_py_binary_search"),
        "must list the valid ids"
    );
}

#[test]
fn duplicate_case_is_rejected_before_any_run() {
    let run = run_smoke(Some("smoke_ts_utils,smoke_ts_utils"));
    assert!(!run.success, "duplicate id must exit non-zero");
    assert!(run.goals.is_empty(), "no case may run on a duplicate id");
}

#[test]
fn empty_selection_is_rejected_before_any_run() {
    for value in ["", "smoke_py_binary_search,"] {
        let run = run_smoke(Some(value));
        assert!(!run.success, "SMOKE_CASES={value:?} must exit non-zero");
        assert!(
            run.goals.is_empty(),
            "no case may run for SMOKE_CASES={value:?}"
        );
        let output = strip_ansi(&combined(&run));
        let valid_ids_line = output
            .lines()
            .find(|line| line.starts_with("Valid ids:"))
            .unwrap_or_else(|| {
                panic!(
                    "rejection for SMOKE_CASES={value:?} must print valid ids; output={output:?}"
                )
            });
        for id in IDS {
            assert!(
                valid_ids_line.split_whitespace().any(|listed| listed == id),
                "valid id {id:?} missing for SMOKE_CASES={value:?}: {valid_ids_line:?}"
            );
        }
    }
}

#[test]
fn partial_run_is_labelled_and_never_matches_release_gate_regex() {
    let run = run_smoke(Some("smoke_py_binary_search"));
    assert!(run.success);
    let text = strip_ansi(&combined(&run));
    assert!(text.contains("PARTIAL RUN"), "partial run must be labelled");
    // Same pattern as scripts/regression_gate.sh (full mode, smoke check).
    let gate = Regex::new("12 / 12|12/12|النجاح:[[:space:]]+100%").unwrap();
    assert!(
        !gate.is_match(&text),
        "partial output must not satisfy the full-gate smoke regex"
    );
}
