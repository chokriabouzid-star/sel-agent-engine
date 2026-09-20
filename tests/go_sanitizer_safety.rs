//! Phase 1 (roadmap 2026-09-16, §4) — Go sanitizer SAFETY regressions.
//!
//! Contract under test: a sanitizer must NEVER change valid Go, and the
//! line-targeted fix must NEVER destroy legitimate escapes.
//! Provenance: all fixtures are CONSTRUCTED from the roadmap's four cases
//! (not bench artifacts). `safety_0` proves with gofmt that the "valid"
//! fixtures really are valid Go BEFORE any sanitizer touches them.
//!
//! Status: expected to FAIL on HEAD 9487e9a — this is the Reproduced-stage
//! evidence; the fix comes in a later step. Do not weaken these tests.

use sel_agent::executor::sanitizers::{
    fix_go_backslashes, fix_go_escaped_quotes, fix_go_missing_package, unescape_go_quotes_on_line,
};

/// Case 1 — valid Go. Inside a raw string (backticks) `\` is an ordinary byte.
/// The file has zero unescaped `"`, so the whole-file rule fires and changes the
/// constant's VALUE from `\"` to `"` (still compiles = silent data change).
const RAW_STRING_GO: &str = "package example\n\nconst payload = `\\\"`\n";

/// Case 2 — valid Go. `"C:\\"` = escaped backslash + closing quote (Windows
/// path). `fix_go_backslashes` collapses `\\"` into `\"` -> unterminated string.
const WINDOWS_PATH_GO: &str = "package example\n\nconst dir = \"C:\\\\\"\n";

/// Case 3 — valid Go. Escaped quotes that live only inside a line comment.
/// Zero unescaped `"` in the file, so the whole-file rule rewrites the comment.
const COMMENT_GO: &str =
    "package example\n\n// note: the model wrote \\\" here on purpose \\\"\nvar x = 1\n";

/// Case 4 — INVALID by construction: line 3 carries a real artifact (`\"a\"`
/// outside any literal) AND a legitimate escaped quote inside a proper string
/// literal on the SAME line.
const MIXED_LINE_GO: &str =
    "package example\n\nfunc f() string { return \\\"a\\\" + \"say \\\"hi\\\"\" }\n";

/// The only correct repair of `MIXED_LINE_GO` (valid Go, checked by gofmt).
const MIXED_LINE_EXPECTED_GO: &str =
    "package example\n\nfunc f() string { return \"a\" + \"say \\\"hi\\\"\" }\n";

fn find_gofmt() -> Option<std::path::PathBuf> {
    if std::process::Command::new("gofmt")
        .arg("-h")
        .output()
        .is_ok()
    {
        return Some(std::path::PathBuf::from("gofmt"));
    }
    let out = std::process::Command::new("go")
        .args(["env", "GOROOT"])
        .output()
        .ok()?;
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let candidate = std::path::Path::new(&root).join("bin").join("gofmt");
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

fn gofmt_accepts(gofmt: &std::path::Path, name: &str, src: &str) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("sel_go_sanitizer_safety_{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(name);
    std::fs::write(&file, src).map_err(|e| e.to_string())?;
    let out = std::process::Command::new(gofmt)
        .args(["-e", "-l"])
        .arg(&file)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{}: {}",
            file.display(),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// Mirrors the `.go` branch of `SafeExecutor::write_file`
/// (src/executor/file_ops.rs @ 9487e9a): missing_package -> escaped_quotes -> backslashes.
fn write_file_go_pipeline(src: &str) -> String {
    let s = fix_go_missing_package(src, "main");
    let s = fix_go_escaped_quotes(&s).unwrap_or(s);
    fix_go_backslashes(&s)
}

#[test]
fn safety_0_valid_fixtures_really_are_valid_go() {
    let Some(gofmt) = find_gofmt() else {
        eprintln!("skip: gofmt not available");
        return;
    };
    for (name, src) in [
        ("case1_raw_string.go", RAW_STRING_GO),
        ("case2_windows_path.go", WINDOWS_PATH_GO),
        ("case3_comment.go", COMMENT_GO),
        ("case4_expected.go", MIXED_LINE_EXPECTED_GO),
    ] {
        if let Err(msg) = gofmt_accepts(&gofmt, name, src) {
            panic!("{name} must be valid Go before any sanitizer runs; gofmt said:\n{msg}");
        }
    }
}

#[test]
fn safety_1_raw_string_backslash_is_data_not_an_artifact() {
    assert_eq!(
        fix_go_escaped_quotes(RAW_STRING_GO),
        None,
        "whole-file rule must not rewrite a raw string literal"
    );
}

#[test]
fn safety_2_windows_path_survives_backslash_fixer() {
    assert_eq!(
        fix_go_backslashes(WINDOWS_PATH_GO),
        WINDOWS_PATH_GO,
        "escaped backslash + closing quote must not be collapsed"
    );
}

#[test]
fn safety_3_comment_only_escapes_are_left_alone() {
    assert_eq!(
        fix_go_escaped_quotes(COMMENT_GO),
        None,
        "escaped quotes inside a comment are not a JSON artifact"
    );
}

#[test]
fn safety_4_line_fix_keeps_legit_escapes_on_the_same_line() {
    let fixed = unescape_go_quotes_on_line(MIXED_LINE_GO, 3)
        .expect("line 3 contains an artifact, so the line fixer must fire");
    assert_eq!(
        fixed, MIXED_LINE_EXPECTED_GO,
        "artifact removed but legitimate escapes destroyed"
    );
}

#[test]
fn safety_5_write_file_pipeline_is_byte_identity_on_valid_go() {
    for (name, src) in [
        ("case1", RAW_STRING_GO),
        ("case2", WINDOWS_PATH_GO),
        ("case3", COMMENT_GO),
    ] {
        assert_eq!(
            write_file_go_pipeline(src),
            src,
            "{name}: valid Go must reach disk byte-for-byte"
        );
    }
}
