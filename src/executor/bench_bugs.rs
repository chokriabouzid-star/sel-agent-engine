#[cfg(test)]
#[allow(clippy::module_inception)]
mod bench_bugs {
    use crate::executor::sanitizers::sanitize_code;
    use crate::executor::SafeExecutor;
    use std::fs;
    use std::path::PathBuf;

    pub fn make_ws(name: &str) -> PathBuf {
        let ws = std::env::temp_dir().join(format!("sel_bench_{}", name));
        let _ = fs::remove_dir_all(&ws);
        fs::create_dir_all(&ws).unwrap();
        ws
    }

    /// B1: sanitize_code  Unicode Quotes  ASCII
    #[test]
    pub fn b1_sanitize_unicode_quotes() {
        let input = "\u{201C}hello\u{201D}"; // "hello" (Unicode)
        let result = sanitize_code(input);
        assert_eq!(result, "\"hello\"", "Unicode quotes must become ASCII");
    }

    /// B2: patch_file     search block
    #[test]
    pub fn b2_patch_explicit_fail() {
        let ws = make_ws("b2");
        fs::write(ws.join("main.rs"), "fn main() {}").unwrap();

        let exec = SafeExecutor::new(ws.clone(), 60);
        let result = exec.patch_file("main.rs", "GHOST_TEXT", "NEW").unwrap();

        assert!(
            !result.success,
            "patch_file must fail when search block is missing"
        );
        assert!(
            result.stderr.contains("not found") || result.stderr.contains("FAILED"),
            "Error should mention 'not found', got: {}",
            result.stderr
        );
    }

    /// B4-a: Language Guard  write_file  .py  Rust workspace
    #[test]
    pub fn b4_language_wall_write() {
        let ws = make_ws("b4w");
        fs::write(ws.join("Cargo.toml"), "[package]\nname=\"t\"\n").unwrap();

        let exec = SafeExecutor::new(ws, 60);
        let result = exec.write_file("calc.py", "x = 1").unwrap();

        assert!(
            !result.success,
            "write_file must reject .py in Rust workspace"
        );
        assert!(
            result.stderr.contains("LANGUAGE LOCK") || result.stderr.contains("BLOCKED"),
            "Must contain LANGUAGE LOCK, got: {}",
            result.stderr
        );
    }

    /// B4-b: Language Guard  patch_file  .py  Go workspace
    #[test]
    pub fn b4_language_wall_patch() {
        let ws = make_ws("b4p");
        fs::write(ws.join("go.mod"), "module test\n").unwrap();
        fs::write(ws.join("calc.py"), "x = 1\n").unwrap();

        let exec = SafeExecutor::new(ws, 60);
        let result = exec.patch_file("calc.py", "x = 1", "x = 2").unwrap();

        assert!(
            !result.success,
            "patch_file must reject .py in Go workspace"
        );
        assert!(
            result.stderr.contains("LANGUAGE LOCK") || result.stderr.contains("BLOCKED"),
            "Must contain LANGUAGE LOCK, got: {}",
            result.stderr
        );
    }
}

//
// AutoFix v7.6.1: Rust &str  String
//
