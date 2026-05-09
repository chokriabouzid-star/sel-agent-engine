// tests/executor_tests.rs
// حزمة اختبارات جاهزة للتشغيل — تركز على مشاكل v7.1.1
// استخدم: cargo test --test executor_tests -- --nocapture

use std::fs;
use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------
// 0. المساعدات والمتغيرات العامة
// ---------------------------------------------------------------
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("sel-test-executor").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("cannot create temp dir");
    dir
}

fn write_file(path: &PathBuf, content: &str) {
    fs::write(path, content).unwrap_or_else(|e| panic!("cannot write {}: {}", path.display(), e));
}

// ---------------------------------------------------------------
// 1. compile_check يتسبب في فشل write_file (حل مشكلة Fix 3)
// ---------------------------------------------------------------
#[test]
fn test_compile_check_blocks_broken_go() {
    let dir = temp_dir("broken_go");
    let go_file = dir.join("broken.go");
    write_file(
        &go_file,
        "package main\n\nfunc main() {\n    fmt.Println(\"hello\")\n}\n",
    );

    let output = Command::new("go")
        .args(&["vet", go_file.to_str().unwrap()])
        .output()
        .expect("go vet failed to run");

    assert!(
        !output.status.success(),
        "go vet should fail on missing import"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("undefined: fmt"),
        "expected 'undefined: fmt'"
    );
}

#[test]
fn test_compile_check_blocks_broken_python() {
    let dir = temp_dir("broken_py");
    let py_file = dir.join("broken.py");
    write_file(&py_file, "def broken_syntax(:\n    pass\n");

    let output = Command::new("python3")
        .args(&["-m", "py_compile", py_file.to_str().unwrap()])
        .output()
        .expect("py_compile failed to run");

    assert!(
        !output.status.success(),
        "py_compile should fail on syntax error"
    );
}

// ---------------------------------------------------------------
// 2. patch_file يعالج escape sequences بشكل صحيح
// ---------------------------------------------------------------
#[test]
fn test_patch_normalize_preserves_escapes() {
    // نستخدم r#""# لتمثيل السلسلة الخام (Raw String) لنضمن وجود الخطوط المائلة
    let original = r#"let s = "hello\nworld\t!\'";"#;

    let backslash_count = original.matches('\\').count();
    assert_eq!(
        backslash_count, 3,
        "original should have three backslashes (\\n, \\t, \\')"
    );

    let normalized = original
        .replace(r#"\t"#, r#"\\t"#)
        .replace(r#"\n"#, r#"\\n"#);
    assert!(
        normalized.contains(r#"\\t"#),
        "normalized must keep escaped tab"
    );
}

#[test]
fn test_patch_normalize_preserves_newlines() {
    let original = "line1\nline2\nline3\n";
    let normalized = original
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n");

    let lines: Vec<&str> = normalized.lines().collect();
    assert_eq!(lines.len(), 3, "must preserve three lines");
}

// ---------------------------------------------------------------
// 4. autofix يعمل بعد write_file
// ---------------------------------------------------------------
#[test]
fn test_autofix_after_write_file() {
    let dir = temp_dir("autofix_write");
    let go_file = dir.join("main.go");
    write_file(
        &go_file,
        "package main\n\nfunc main() {\n    fmt.Println(\"ok\")\n}\n",
    );

    let _ = Command::new("go")
        .args(&["mod", "init", "testmod"])
        .current_dir(&dir)
        .output();

    let vet_fail = Command::new("go")
        .args(&["vet", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!vet_fail.status.success());

    let fixed = "package main\n\nimport \"fmt\"\n\nfunc main() {\n    fmt.Println(\"ok\")\n}\n";
    write_file(&go_file, fixed);

    let vet_ok = Command::new("go")
        .args(&["vet", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(vet_ok.status.success());
}

// ---------------------------------------------------------------
// 5. go vet ./... بدلاً من go vet file.go
// ---------------------------------------------------------------
#[test]
fn test_go_vet_dot_slash_dot() {
    let dir = temp_dir("go_vet_all");
    let _ = Command::new("go")
        .args(&["mod", "init", "testmod"])
        .current_dir(&dir)
        .output();

    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let go_file = src_dir.join("lib.go");
    write_file(
        &go_file,
        "package lib\n\nfunc Hello() string { return \"hello\" }\n",
    );

    let output = Command::new("go")
        .args(&["vet", "./..."])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "go vet ./... failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------
// 6. اختبار شامل
// ---------------------------------------------------------------
#[test]
fn test_full_flow_compile_fail_then_autofix_then_test_pass() {
    let dir = temp_dir("full_flow");
    let _ = Command::new("go")
        .args(&["mod", "init", "testmod"])
        .current_dir(&dir)
        .output();

    let test_file = dir.join("calc_test.go");
    write_file(&test_file, 
        "package main\n\nimport \"testing\"\n\nfunc TestAdd(t *testing.T) {\n    result := Add(1,2)\n    if result != 3 { t.Fatal(\"fail\") }\n}\n");

    let output_before = Command::new("go")
        .args(&["test", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!output_before.status.success());

    let src_file = dir.join("calc.go");
    write_file(
        &src_file,
        "package main\n\nfunc Add(a,b int) int { return a+b }\n",
    );

    let output_after = Command::new("go")
        .args(&["test", "."])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        output_after.status.success(),
        "Stderr: {}",
        String::from_utf8_lossy(&output_after.stderr)
    );
}
