//! Phase 3b (roadmap 2026-09-16, §6) — Oracle flag-preservation regressions.
//!
//! Contract under test: when the caller (LLM plan / recorded trajectory) asks for a
//! SPECIFIC test invocation, the Oracle must honour its flags and package scope
//! instead of collapsing everything into one hard-coded default command.
//!
//! Status on HEAD 29b12f1: every `bug_*` and `e2e_bug_*` test is expected to FAIL.
//! Every `guard_*` test MUST already pass — they pin the current behaviour that the
//! upcoming fix is not allowed to change.

use sel_agent::executor::SafeExecutor;
use sel_agent::workspace_oracle::{ProjectType, WorkspaceOracle};
use std::path::PathBuf;

// ---------------------------------------------------------------- helpers ----

fn fresh_ws(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("sel_p3b_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("ws dir");
    p
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(f)
}

fn tool_available(tool: &str, arg: &str) -> bool {
    std::process::Command::new(tool)
        .arg(arg)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn go_ws(tag: &str) -> PathBuf {
    let ws = fresh_ws(tag);
    std::fs::write(ws.join("go.mod"), "module twopkg\n\ngo 1.21\n").expect("go.mod");
    for pkg in ["pkga", "pkgb"] {
        let dir = ws.join(pkg);
        std::fs::create_dir_all(&dir).expect("pkg dir");
        std::fs::write(
            dir.join("src.go"),
            format!("package {}\n\nfunc Val() int {{ return 1 }}\n", pkg),
        )
        .expect("src.go");
        std::fs::write(
            dir.join("src_test.go"),
            format!(
                "package {}\n\nimport \"testing\"\n\nfunc TestValue{}(t *testing.T) {{\n\tif Val() != 1 {{\n\t\tt.Fatal(\"bad\")\n\t}}\n}}\n",
                pkg,
                pkg
            ),
        )
        .expect("src_test.go");
    }
    ws
}

fn cargo_ws(tag: &str) -> PathBuf {
    let ws = fresh_ws(tag);
    // Root package has TWO tests. `exclude` keeps mylib a standalone package so
    // cargo never raises "believes it's in a workspace" — fully deterministic.
    std::fs::write(
        ws.join("Cargo.toml"),
        "[package]\nname = \"rootpkg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\nexclude = [\"mylib\"]\n",
    )
    .expect("root Cargo.toml");
    std::fs::create_dir_all(ws.join("src")).expect("src");
    std::fs::write(
        ws.join("src/lib.rs"),
        "pub fn one() -> i32 { 1 }\n\n#[cfg(test)]\nmod t {\n    #[test]\n    fn root_a() { assert_eq!(super::one(), 1); }\n    #[test]\n    fn root_b() { assert_eq!(super::one(), 1); }\n}\n",
    )
    .expect("root lib.rs");

    // Nested package has exactly ONE test.
    std::fs::create_dir_all(ws.join("mylib/src")).expect("mylib/src");
    std::fs::write(
        ws.join("mylib/Cargo.toml"),
        "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("mylib Cargo.toml");
    std::fs::write(
        ws.join("mylib/src/lib.rs"),
        "pub fn two() -> i32 { 2 }\n\n#[cfg(test)]\nmod t {\n    #[test]\n    fn mylib_only() { assert_eq!(super::two(), 2); }\n}\n",
    )
    .expect("mylib lib.rs");
    ws
}

// =============================================================== BUG TESTS ====
// (bodies 1 and 2 are byte-identical to the run that produced the RED evidence)

#[test]
fn bug_go_resolve_drops_flags_in_target() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("go.mod"), "module test\n").unwrap();

    let oracle = WorkspaceOracle::new(dir.path().to_path_buf());
    let (prog, args) = oracle.resolve_test_command("go test -race -count=1 ./pkg/...");

    assert_eq!(prog, "go", "program should be go");
    // BUG PROOF: current implementation returns ["test", "./...", "-v"] unconditionally
    assert!(
        args.contains(&"-race".to_string()),
        "GO FLAG BUG: -race was dropped from resolved args: {:?}",
        args
    );
    assert!(
        args.contains(&"-count=1".to_string()),
        "GO FLAG BUG: -count=1 was dropped from resolved args: {:?}",
        args
    );
    assert!(
        args.contains(&"./pkg/...".to_string()),
        "GO FLAG BUG: ./pkg/... scope was dropped from resolved args: {:?}",
        args
    );
}

#[test]
fn bug_rust_resolve_drops_flags_and_ignores_nested_context() {
    let dir = tempfile::tempdir().unwrap();
    let lib_dir = dir.path().join("mylib");
    std::fs::create_dir(&lib_dir).unwrap();
    std::fs::write(
        lib_dir.join("Cargo.toml"),
        "[package]\nname=\"mylib\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir(lib_dir.join("src")).unwrap();
    std::fs::write(lib_dir.join("src/lib.rs"), "").unwrap();

    let oracle = WorkspaceOracle::new(dir.path().to_path_buf());
    // Phase 3a guarantees nested detection works
    assert!(
        matches!(oracle.current_type(), ProjectType::Rust),
        "should detect nested Rust project"
    );

    let (prog, args) = oracle.resolve_test_command("cargo test --lib -p mylib");
    assert_eq!(prog, "cargo", "program should be cargo");
    // BUG PROOF: current implementation returns ["test", "--", "--nocapture"] unconditionally
    assert!(
        args.contains(&"--lib".to_string()),
        "RUST FLAG BUG: --lib was dropped from resolved args: {:?}",
        args
    );
    assert!(
        args.contains(&"-p".to_string()) || args.contains(&"mylib".to_string()),
        "RUST FLAG BUG: -p/mylib was dropped from resolved args: {:?}",
        args
    );
}

#[test]
fn bug_go_run_filter_is_dropped() {
    let ws = fresh_ws("go_runfilter");
    std::fs::write(ws.join("go.mod"), "module t\n").expect("go.mod");
    let oracle = WorkspaceOracle::new(ws);
    let (_, args) = oracle.resolve_test_command("go test -run TestOnlyThis ./...");
    assert!(
        args.contains(&"-run".to_string()) && args.contains(&"TestOnlyThis".to_string()),
        "GO FLAG BUG: -run TestOnlyThis filter was dropped: {:?}",
        args
    );
}

#[test]
fn bug_rust_manifest_path_is_dropped() {
    let ws = fresh_ws("rust_manifest");
    std::fs::write(
        ws.join("Cargo.toml"),
        "[package]\nname=\"r\"\nversion=\"0.1.0\"\n",
    )
    .expect("Cargo.toml");
    let oracle = WorkspaceOracle::new(ws);
    let (_, args) = oracle.resolve_test_command("cargo test --manifest-path mylib/Cargo.toml");
    assert!(
        args.iter()
            .any(|a| a == "--manifest-path" || a.starts_with("--manifest-path=")),
        "RUST FLAG BUG: --manifest-path was dropped: {:?}",
        args
    );
}

// ============================================================= GUARD TESTS ====
// These pin TODAY's behaviour byte-for-byte. The fix must not move them.

fn go_default() -> Vec<String> {
    vec!["test".into(), "./...".into(), "-v".into()]
}

fn rust_default() -> Vec<String> {
    vec!["test".into(), "--".into(), "--nocapture".into()]
}

#[test]
fn guard_go_bare_targets_keep_default_triple() {
    let ws = fresh_ws("guard_go_bare");
    std::fs::write(ws.join("go.mod"), "module t\n").expect("go.mod");
    let oracle = WorkspaceOracle::new(ws);
    for target in ["go", "go test", "go test ./...", "", "auto"] {
        let (prog, args) = oracle.resolve_test_command(target);
        assert_eq!(prog, "go", "target {:?}", target);
        assert_eq!(
            args,
            go_default(),
            "target {:?} must keep the default",
            target
        );
    }
}

#[test]
fn guard_go_shell_pipeline_falls_back_to_default() {
    let ws = fresh_ws("guard_go_pipe");
    std::fs::write(ws.join("go.mod"), "module t\n").expect("go.mod");
    let oracle = WorkspaceOracle::new(ws);
    for target in ["go test ./... && echo ok", "go test ./... | tee out.txt"] {
        let (prog, args) = oracle.resolve_test_command(target);
        assert_eq!(prog, "go", "target {:?}", target);
        assert_eq!(
            args,
            go_default(),
            "shell metachars must not be forwarded: {:?}",
            target
        );
    }
}

#[test]
fn guard_go_non_test_subcommand_falls_back_to_default() {
    let ws = fresh_ws("guard_go_vet");
    std::fs::write(ws.join("go.mod"), "module t\n").expect("go.mod");
    let oracle = WorkspaceOracle::new(ws);
    let (prog, args) = oracle.resolve_test_command("go vet ./...");
    assert_eq!(prog, "go");
    assert_eq!(
        args,
        go_default(),
        "`go vet` must not become the test command"
    );
}

#[test]
fn guard_rust_bare_targets_keep_default_triple() {
    let ws = fresh_ws("guard_rust_bare");
    std::fs::write(
        ws.join("Cargo.toml"),
        "[package]\nname=\"r\"\nversion=\"0.1.0\"\n",
    )
    .expect("Cargo.toml");
    let oracle = WorkspaceOracle::new(ws);
    for target in ["cargo", "cargo test", "", "auto", "pytest -v"] {
        let (prog, args) = oracle.resolve_test_command(target);
        assert_eq!(prog, "cargo", "target {:?}", target);
        assert_eq!(
            args,
            rust_default(),
            "target {:?} must keep the default",
            target
        );
    }
}

#[test]
fn guard_node_arm_is_untouched() {
    let ws = fresh_ws("guard_node");
    std::fs::write(ws.join("package.json"), "{\"name\":\"n\"}\n").expect("package.json");
    let oracle = WorkspaceOracle::new(ws);

    let (prog, args) = oracle.resolve_test_command("npm test");
    assert_eq!(prog, "npx");
    assert_eq!(args, vec!["jest", "--runInBand", "--forceExit"]);

    let (prog, args) = oracle.resolve_test_command("auto");
    assert_eq!(prog, "npm");
    assert_eq!(args, vec!["test", "--", "--runInBand", "--forceExit"]);
}

#[test]
fn guard_python_arm_is_untouched() {
    let ws = fresh_ws("guard_py");
    std::fs::write(ws.join("requirements.txt"), "pytest\n").expect("requirements.txt");
    let oracle = WorkspaceOracle::new(ws);
    for target in ["pytest", "pytest -k foo", "auto", ""] {
        let (prog, args) = oracle.resolve_test_command(target);
        assert_eq!(prog, "pytest", "target {:?}", target);
        assert_eq!(args, vec!["-v", "--tb=short"], "target {:?}", target);
    }
}

// ========================================================== E2E BEHAVIOUR ====
// Proof that the resolved command is what actually runs (the log line must not lie).

#[test]
fn e2e_guard_go_default_runs_both_packages() {
    if !tool_available("go", "version") {
        eprintln!("skip: go toolchain not available");
        return;
    }
    let exec = SafeExecutor::new(go_ws("e2e_go_all"), 180);
    let r = block_on(exec.run_tests("go")).expect("run_tests must return Ok");
    assert!(r.success, "default go run must pass: {}", r.stderr);
    assert_eq!(
        r.stdout, "2 passed, 0 failed",
        "default must run BOTH packages"
    );
}

#[test]
fn e2e_bug_go_package_scope_is_ignored() {
    if !tool_available("go", "version") {
        eprintln!("skip: go toolchain not available");
        return;
    }
    let exec = SafeExecutor::new(go_ws("e2e_go_scope"), 180);
    let r = block_on(exec.run_tests("go test ./pkga/...")).expect("run_tests must return Ok");
    assert_eq!(
        r.stdout, "1 passed, 0 failed",
        "GO SCOPE BUG: ./pkga/... must run ONE package, but the runner executed: {} (stderr: {})",
        r.stdout, r.stderr
    );
}

#[test]
fn e2e_guard_cargo_default_runs_root_package() {
    if !tool_available("cargo", "--version") {
        eprintln!("skip: cargo not available");
        return;
    }
    let exec = SafeExecutor::new(cargo_ws("e2e_cargo_root"), 300);
    let r = block_on(exec.run_tests("cargo")).expect("run_tests must return Ok");
    assert!(r.success, "default cargo run must pass: {}", r.stderr);
    assert_eq!(
        r.stdout, "2 passed, 0 failed",
        "default must run the ROOT package"
    );
}

#[test]
fn e2e_bug_cargo_manifest_path_is_ignored() {
    if !tool_available("cargo", "--version") {
        eprintln!("skip: cargo not available");
        return;
    }
    let exec = SafeExecutor::new(cargo_ws("e2e_cargo_nested"), 300);
    let r = block_on(exec.run_tests("cargo test --manifest-path mylib/Cargo.toml"))
        .expect("run_tests must return Ok");
    assert_eq!(
        r.stdout, "1 passed, 0 failed",
        "CARGO MANIFEST BUG: --manifest-path mylib/Cargo.toml must run ONLY mylib (1 test), \
         but the runner executed the root package instead: {} (stderr: {})",
        r.stdout, r.stderr
    );
}
