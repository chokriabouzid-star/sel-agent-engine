//! W5 regression — Node Oracle must respect package.json `scripts.test`.
//!
//! RED on current HEAD: Node resolution ignores `scripts.test` and either
//! forces `npx jest --runInBand --forceExit` or injects Jest-only flags into
//! `npm test`.

use sel_agent::executor::SafeExecutor;
use sel_agent::workspace_oracle::WorkspaceOracle;
use std::path::PathBuf;

fn fresh_ws(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("sel_w5_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create workspace");
    path
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(future)
}

fn tool_available(tool: &str, arg: &str) -> bool {
    std::process::Command::new(tool)
        .arg(arg)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn write_node_test_project(workspace: &std::path::Path) {
    std::fs::write(
        workspace.join("package.json"),
        r#"{"name":"w5-node-script","private":true,"scripts":{"test":"node --test"}}"#,
    )
    .expect("write package.json");

    std::fs::write(
        workspace.join("math.test.js"),
        "const test = require('node:test');\n\
         const assert = require('node:assert/strict');\n\
         test('adds', () => { assert.equal(1 + 1, 2); });\n",
    )
    .expect("write math.test.js");
}

#[test]
fn oracle_uses_package_scripts_test_for_explicit_npm_test() {
    let workspace = fresh_ws("oracle_explicit");
    write_node_test_project(&workspace);

    let oracle = WorkspaceOracle::new(workspace);
    let (program, args) = oracle.resolve_test_command("npm test");

    assert_eq!(program, "npm");
    assert_eq!(
        args,
        vec!["test"],
        "must use package.json scripts.test without forcing Jest or injecting Jest flags"
    );
}

#[test]
fn oracle_uses_package_scripts_test_for_auto_target() {
    let workspace = fresh_ws("oracle_auto");
    write_node_test_project(&workspace);

    let oracle = WorkspaceOracle::new(workspace);
    let (program, args) = oracle.resolve_test_command("auto");

    assert_eq!(program, "npm");
    assert_eq!(
        args,
        vec!["test"],
        "auto must respect package.json scripts.test without Jest flags"
    );
}

#[test]
fn oracle_fallback_without_scripts_test_is_unchanged() {
    let workspace = fresh_ws("fallback");
    std::fs::write(
        workspace.join("package.json"),
        r#"{"name":"fallback","private":true}"#,
    )
    .expect("write package.json");

    let oracle = WorkspaceOracle::new(workspace);

    let (program, args) = oracle.resolve_test_command("npm test");
    assert_eq!(program, "npx");
    assert_eq!(args, vec!["jest", "--runInBand", "--forceExit"]);

    let (program, args) = oracle.resolve_test_command("auto");
    assert_eq!(program, "npm");
    assert_eq!(args, vec!["test", "--", "--runInBand", "--forceExit"]);
}

#[test]
fn e2e_package_scripts_node_test_runs_successfully() {
    if !tool_available("node", "--version") || !tool_available("npm", "--version") {
        eprintln!("skip: node/npm not available");
        return;
    }

    let workspace = fresh_ws("e2e_node_test");
    write_node_test_project(&workspace);

    let executor = SafeExecutor::new(workspace, 60);

    // Use `auto`, not explicit `npm test`: current HEAD then runs npm locally
    // with injected Jest flags. This produces deterministic offline RED and
    // never gives `npx` an opportunity to download Jest.
    let result = block_on(executor.run_tests("auto")).expect("run_tests must return Ok");

    assert!(
        result.success,
        "package scripts.test should succeed; stdout={} stderr={}",
        result.stdout, result.stderr
    );
    assert_eq!(result.stdout, "1 passed, 0 failed");
}
