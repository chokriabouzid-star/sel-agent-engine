// src/bench_realworld.rs — v7.5 Feature-Targeted Benchmark
// يختبر: Compile-First | quick_fix | Language Guard | Real-World Patterns

use crate::agent;
use crate::types;
use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::{Duration, Instant};

struct BenchCase {
    name: &'static str,
    goal: &'static str,
    lang: &'static str,
    tier: u8,
    tests_feature: &'static str,
    reference_tests: Option<(&'static str, &'static str)>,
    scaffold_files: Vec<(&'static str, &'static str)>,
}

impl BenchCase {
    fn new(
        name: &'static str,
        goal: &'static str,
        lang: &'static str,
        tier: u8,
        tests_feature: &'static str,
    ) -> Self {
        Self {
            name,
            goal,
            lang,
            tier,
            tests_feature,
            reference_tests: None,
            scaffold_files: vec![],
        }
    }

    fn with_tests(mut self, filename: &'static str, content: &'static str) -> Self {
        self.reference_tests = Some((filename, content));
        self
    }

    fn with_scaffold(mut self, path: &'static str, content: &'static str) -> Self {
        self.scaffold_files.push((path, content));
        self
    }
}

pub async fn run_bench_realworld(
    _api_key: &str,
    tier: Option<u8>,
    max_repairs: u8,
    record: bool,
    replay: bool,
    rerecord: bool,
    delay: u64,
    skip_recorded: bool,
    focus: &[String],
) -> Result<()> {
    println!("\n╔═══════════════════════════════════════════════════════════════════╗");
    println!("║   SEL Agent v8.2.0 — Feature-Targeted Benchmark                  ║");
    println!("║   Compile-First | quick_fix | Language Guard | Real-World         ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝\n");

    let all_cases = build_cases();

    // 1. Filter by tier
    let cases: Vec<_> = if let Some(t) = tier {
        all_cases.into_iter().filter(|c| c.tier == t).collect()
    } else {
        all_cases
    };

    // 2. Filter by focus
    let cases: Vec<_> = if focus.is_empty() {
        cases
    } else {
        cases
            .into_iter()
            .filter(|c| {
                focus.iter().any(|f| {
                    c.name.to_lowercase().contains(&f.to_lowercase())
                        || c.tests_feature.to_lowercase().contains(&f.to_lowercase())
                        || c.lang.to_lowercase().contains(&f.to_lowercase())
                })
            })
            .collect()
    };

    if cases.is_empty() {
        println!("No cases matched the given filters.");
        return Ok(());
    }

    print_test_plan(&cases);

    // Print active flags
    if replay  { println!("   Mode:         🔄 REPLAY{}",  if rerecord { " + auto-rerecord on fail" } else { "" }); }
    if record  { println!("   Mode:         ⏺  RECORD"); }
    if skip_recorded { println!("   skip-recorded: enabled"); }
    println!("   Cooldown:      {}s between cases\n", delay);

    let total = cases.len();
    let mut passed = 0usize;
    let mut healed = 0usize;
    let mut total_repairs = 0usize;
    let mut feature_stats: std::collections::HashMap<&str, (usize, usize)> =
        std::collections::HashMap::new();

    let start_time = Instant::now();
    let tmpdir = std::env::temp_dir();

    for (i, case) in cases.iter().enumerate() {
        // Trajectory path — stable slug per case name
        let traj_slug = case
            .name
            .to_lowercase()
            .replace(" ", "_")
            .replace(":", "")
            .replace("/", "_");
        let traj_dir = std::env::current_dir()?
            .join("fixtures")
            .join("trajectories")
            .join(&traj_slug);

        // 3. skip-recorded: skip cases whose trajectory directory already exists
        if skip_recorded && traj_dir.exists() {
            println!(
                "  {} T{} [{}] {} — skipped (trajectory exists)",
                "⏭".yellow(),
                case.tier,
                case.lang.blue(),
                case.name.bold()
            );
            continue;
        }

        let workspace = tmpdir.join(format!("sel-bench-rw-{}-{}", i, std::process::id()));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace)?;

        // Write reference tests
        if let Some((filename, content)) = case.reference_tests {
            let test_path = workspace.join(filename);
            if let Some(parent) = test_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&test_path, content)?;
        }

        // Write scaffold files
        for (path, content) in &case.scaffold_files {
            let full_path = workspace.join(path);
            if let Some(parent) = full_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&full_path, content)?;
        }

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!(
                    "{{spinner:.yellow}} [{}/{}] T{} [{}] {}...",
                    i + 1,
                    total,
                    case.tier,
                    case.lang,
                    case.name
                ))
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.enable_steady_tick(Duration::from_millis(80));

        let live = crate::llm::live::LiveProvider::from_env();
        let provider: Box<dyn crate::llm::LLMProvider> = if replay {
            Box::new(crate::llm::replay::ReplayProvider::new(&traj_dir))
        } else if record {
            Box::new(crate::llm::record::RecorderProvider::new(
                Box::new(live),
                &traj_dir,
            ))
        } else {
            Box::new(live)
        };

        let mut ag = agent::Agent::new_with_model(
            String::new(),
            String::new(),
            workspace.clone(),
            case.goal.to_string(),
            max_repairs,
            types::ContextConfig::default(),
            provider,
        );
        ag.ctx.skip_mutation = true;
        ag.bench_mode = true;

        let case_start = Instant::now();
        let run_res = ag.run().await;
        let case_dur = case_start.elapsed();
        pb.finish_and_clear();

        let entry = feature_stats.entry(case.tests_feature).or_insert((0, 0));
        entry.1 += 1;

        // Determine success
        let mut case_ok = match &run_res {
            Ok(_) => ag.is_success(),
            Err(_) => false,
        };

        // 4. rerecord: if replay failed, re-run with LiveProvider + RecorderProvider
        if replay && !case_ok && rerecord {
            println!(
                "   ⚠️  [{}] replay failed — auto-rerecording...",
                case.name
            );

            // Re-scaffold workspace (was cleaned by run)
            let _ = std::fs::remove_dir_all(&workspace);
            std::fs::create_dir_all(&workspace)?;
            if let Some((filename, content)) = case.reference_tests {
                let test_path = workspace.join(filename);
                if let Some(parent) = test_path.parent() { let _ = std::fs::create_dir_all(parent); }
                std::fs::write(&test_path, content)?;
            }
            for (path, content) in &case.scaffold_files {
                let full_path = workspace.join(path);
                if let Some(parent) = full_path.parent() { let _ = std::fs::create_dir_all(parent); }
                std::fs::write(&full_path, content)?;
            }

            let live2 = crate::llm::live::LiveProvider::from_env();
            let recorder = Box::new(crate::llm::record::RecorderProvider::new(
                Box::new(live2),
                &traj_dir,
            ));
            let mut heal_ag = agent::Agent::new_with_model(
                String::new(),
                String::new(),
                workspace.clone(),
                case.goal.to_string(),
                max_repairs,
                types::ContextConfig::default(),
                recorder,
            );
            heal_ag.ctx.skip_mutation = true;
            heal_ag.bench_mode = true;

            let _ = heal_ag.run().await;
            case_ok = heal_ag.is_success();
            if case_ok {
                healed += 1;
                println!("   ✅ [{}] auto-rerecorded successfully.", case.name);
            } else {
                println!("   ❌ [{}] auto-rerecord also failed.", case.name);
            }
        }

        // Print result
        let repairs = ag.repair_count();
        if case_ok {
            total_repairs += repairs;
            passed += 1;
            entry.0 += 1;
            println!(
                "  {} T{} [{}] {} ({}s, {} repairs) | {}",
                "✅".green(),
                case.tier,
                case.lang.blue(),
                case.name.bold(),
                case_dur.as_secs(),
                repairs,
                case.tests_feature.magenta()
            );
        } else {
            let err_str = match run_res {
                Err(e) => format!(" ERR: {}", e.to_string().chars().take(60).collect::<String>()),
                Ok(_) => String::new(),
            };
            println!(
                "  {} T{} [{}] {} ({}s, {} repairs){} | {}",
                "❌".red(),
                case.tier,
                case.lang.blue(),
                case.name.bold(),
                case_dur.as_secs(),
                repairs,
                err_str,
                case.tests_feature.magenta()
            );
        }

        let _ = std::fs::remove_dir_all(&workspace);

        if i < total - 1 {
            if delay > 0 {
                println!("     ⏳ {}s cooldown...", delay);
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        }
    }

    print_results(
        passed,
        total,
        total_repairs,
        healed,
        start_time.elapsed(),
        &feature_stats,
        tier,
    );

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Cases — مصممة لاختبار ميزات v7.5
// ═══════════════════════════════════════════════════════════════════════════

fn build_cases() -> Vec<BenchCase> {
    vec![
        // ────────────────────────────────────────────────────────────
        // TIER 1: Compile-First Pipeline
        // هدف: التحقق أن compile check يعمل لكل لغة
        // ────────────────────────────────────────────────────────────
        BenchCase::new(
            "Compile-Check Python",
            "Create a Python module 'calculator.py' with functions: add(a,b), subtract(a,b), multiply(a,b), divide(a,b). divide must raise ValueError if b is zero.\n\nTests: python -m pytest test_calculator.py -v",
            "Python",
            1,
            "py_compile",
        )
        .with_tests(
            "test_calculator.py",
            r#"import pytest
from calculator import add, subtract, multiply, divide

def test_add():
    assert add(2, 3) == 5
    assert add(-1, 1) == 0

def test_subtract():
    assert subtract(5, 3) == 2
    assert subtract(0, 5) == -5

def test_multiply():
    assert multiply(3, 4) == 12
    assert multiply(0, 100) == 0

def test_divide():
    assert divide(10, 2) == 5.0
    assert divide(7, 2) == 3.5

def test_divide_by_zero():
    with pytest.raises(ValueError):
        divide(5, 0)
"#,
        ),

        BenchCase::new(
            "Compile-Check Go",
            "Create a Go package 'mathutil' with exported functions: Add, Subtract, Multiply, Divide. Divide returns (float64, error) and returns error if b is zero. Package name must be 'mathutil'.
Tests: go test -v",
            "Go",
            1,
            "go test -run=^$",
        )
        .with_tests(
            "mathutil_test.go",
            r#"package mathutil

import "testing"

func TestAdd(t *testing.T) {
    if Add(2, 3) != 5 { t.Error("Add(2,3) expected 5") }
    if Add(-1, 1) != 0 { t.Error("Add(-1,1) expected 0") }
}

func TestSubtract(t *testing.T) {
    if Subtract(5, 3) != 2 { t.Error("Subtract(5,3) expected 2") }
}

func TestMultiply(t *testing.T) {
    if Multiply(3, 4) != 12 { t.Error("Multiply(3,4) expected 12") }
    if Multiply(0, 5) != 0 { t.Error("Multiply(0,5) expected 0") }
}

func TestDivide(t *testing.T) {
    r, err := Divide(10, 2)
    if err != nil || r != 5 { t.Errorf("Divide(10,2) expected 5, got %v %v", r, err) }
    _, err = Divide(5, 0)
    if err == nil { t.Error("Divide by zero must return error") }
}
"#,
        )
        .with_scaffold("go.mod", "module mathutil\n\ngo 1.21\n"),

        BenchCase::new(
            "Compile-Check Rust",
            "Create a Rust library in src/lib.rs with pub fn add(a:i32,b:i32)->i32, subtract, multiply, and divide(a:f64,b:f64)->Result<f64,String> returning Err if b is zero. Include #[cfg(test)] with tests for all four functions.\n\nTests: cargo test",
            "Rust",
            1,
            "cargo check",
        )
        .with_scaffold(
            "Cargo.toml",
            "[package]\nname = \"mathlib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
        ),

        BenchCase::new(
            "Compile-Check TypeScript",
            "Create a TypeScript file 'utils.ts' exporting: add(a:number,b:number):number, subtract, multiply, divide (throws Error if b is zero). Use explicit TypeScript types throughout.\n\nTests: npx jest utils.test.ts",
            "TypeScript",
            1,
            "tsc --noEmit",
        )
        .with_tests(
            "utils.test.ts",
            r#"import { add, subtract, multiply, divide } from './utils';

describe('utils', () => {
    test('add', () => {
        expect(add(2, 3)).toBe(5);
        expect(add(-1, 1)).toBe(0);
    });
    test('subtract', () => {
        expect(subtract(5, 3)).toBe(2);
    });
    test('multiply', () => {
        expect(multiply(3, 4)).toBe(12);
    });
    test('divide', () => {
        expect(divide(10, 2)).toBe(5);
    });
    test('divide by zero throws', () => {
        expect(() => divide(5, 0)).toThrow();
    });
});
"#,
        ),

        // ────────────────────────────────────────────────────────────
        // TIER 2: quick_fix (إصلاح تلقائي بدون LLM)
        // هدف: AutoFix Go imports + ModuleNotFoundError handling
        // ────────────────────────────────────────────────────────────
        BenchCase::new(
            "QuickFix Go imports",
            "Create a Go program in main.go with package main. Implement func Greet(name string) string that returns 'HELLO, NAME!' in uppercase using strings.ToUpper and fmt.Sprintf. The file must compile and pass the existing tests.",
            "Go",
            2,
            "quick_fix: go import",
        )
        .with_tests(
            "main_test.go",
            r#"package main

import "testing"

func TestGreet(t *testing.T) {
    result := Greet("world")
    if result != "HELLO, WORLD!" {
        t.Errorf("expected 'HELLO, WORLD!' got '%s'", result)
    }
    result2 := Greet("alice")
    if result2 != "HELLO, ALICE!" {
        t.Errorf("expected 'HELLO, ALICE!' got '%s'", result2)
    }
}
"#,
        )
        .with_scaffold("go.mod", "module greeter\n\ngo 1.21\n"),

        BenchCase::new(
            "QuickFix Python requests",
            "Create a Python module 'fetcher.py' with function fetch(url: str) -> dict that uses the 'requests' library to GET the url and returns response.json(). Handle connection errors by returning {'error': str(e)}.\n\nTests: python -m pytest test_fetcher.py -v",
            "Python",
            2,
            "quick_fix: pip install",
        )
        .with_tests(
            "test_fetcher.py",
            r#"from unittest.mock import patch, MagicMock
from fetcher import fetch

@patch('fetcher.requests.get')
def test_fetch_success(mock_get):
    mock_response = MagicMock()
    mock_response.json.return_value = {"key": "value"}
    mock_get.return_value = mock_response
    result = fetch("http://example.com")
    assert result == {"key": "value"}

@patch('fetcher.requests.get')
def test_fetch_error(mock_get):
    import requests
    mock_get.side_effect = requests.exceptions.ConnectionError("connection refused")
    result = fetch("http://bad-url")
    assert "error" in result
"#,
        ),

        BenchCase::new(
            "QuickFix Go strings package",
            "Create a Go file 'textutils.go' in package textutils with exported functions: Reverse(s string) string, CountVowels(s string) int, IsPalindrome(s string) bool. Use only standard library.\n\nTests: go test -v",
            "Go",
            2,
            "quick_fix: go strings",
        )
        .with_tests(
            "textutils_test.go",
            r#"package textutils

import "testing"

func TestReverse(t *testing.T) {
    if Reverse("hello") != "olleh" { t.Error("Reverse failed") }
    if Reverse("") != "" { t.Error("Reverse empty failed") }
    if Reverse("a") != "a" { t.Error("Reverse single failed") }
}

func TestCountVowels(t *testing.T) {
    if CountVowels("hello") != 2 { t.Error("CountVowels hello failed") }
    if CountVowels("rhythm") != 0 { t.Error("CountVowels rhythm failed") }
    if CountVowels("aeiou") != 5 { t.Error("CountVowels aeiou failed") }
}

func TestIsPalindrome(t *testing.T) {
    if !IsPalindrome("racecar") { t.Error("racecar is palindrome") }
    if !IsPalindrome("level") { t.Error("level is palindrome") }
    if IsPalindrome("hello") { t.Error("hello is not palindrome") }
}
"#,
        )
        .with_scaffold("go.mod", "module textutils\n\ngo 1.21\n"),

        // ────────────────────────────────────────────────────────────
        // TIER 3: Language Guard + Bug Fix
        // هدف: الوكيل يُصلح ملفاً موجوداً دون كسر workspace
        // ────────────────────────────────────────────────────────────
        BenchCase::new(
            "BugFix Rust: wrong operator",
            "Fix the bug in src/lib.rs. The add function currently returns a - b instead of a + b. Fix only this bug and run cargo test to verify all tests pass.",
            "Rust",
            3,
            "language_guard: Rust bugfix",
        )
        .with_scaffold(
            "Cargo.toml",
            "[package]\nname = \"bugfix\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
        )
        .with_scaffold(
            "src/lib.rs",
            r#"pub fn add(a: i32, b: i32) -> i32 {
    a - b  // BUG: should be a + b
}

pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_add() {
        assert_eq!(add(2, 3), 5);
        assert_eq!(add(-1, 1), 0);
        assert_eq!(add(0, 0), 0);
    }
    #[test]
    fn test_multiply() {
        assert_eq!(multiply(3, 4), 12);
        assert_eq!(multiply(0, 5), 0);
    }
}
"#,
        ),

        BenchCase::new(
            "BugFix Go: wrong return",
            "Fix the bug in calculator.go: the Multiply function returns a + b instead of a * b. Fix only this function. The existing tests in calculator_test.go must pass.",
            "Go",
            3,
            "language_guard: Go bugfix",
        )
        .with_scaffold("go.mod", "module calculator\n\ngo 1.21\n")
        .with_scaffold(
            "calculator.go",
            r#"package calculator

func Add(a, b int) int {
    return a + b
}

func Multiply(a, b int) int {
    return a + b  // BUG: should be a * b
}
"#,
        )
        .with_scaffold(
            "calculator_test.go",
            r#"package calculator

import "testing"

func TestAdd(t *testing.T) {
    if Add(2, 3) != 5 { t.Error("Add failed") }
}

func TestMultiply(t *testing.T) {
    if Multiply(3, 4) != 12 { t.Errorf("Multiply(3,4): expected 12") }
    if Multiply(0, 5) != 0 { t.Errorf("Multiply(0,5): expected 0") }
    if Multiply(-2, 3) != -6 { t.Errorf("Multiply(-2,3): expected -6") }
}
"#,
        ),

        BenchCase::new(
            "BugFix Python: off-by-one",
            "Fix the bug in stats.py: the average function divides by len(nums)-1 instead of len(nums). Fix this bug. All tests in test_stats.py must pass.",
            "Python",
            3,
            "language_guard: Python bugfix",
        )
        .with_scaffold(
            "stats.py",
            r#"def average(nums):
    if not nums:
        raise ValueError("empty list")
    return sum(nums) / (len(nums) - 1)  # BUG: should divide by len(nums)

def maximum(nums):
    if not nums:
        raise ValueError("empty list")
    return max(nums)

def minimum(nums):
    if not nums:
        raise ValueError("empty list")
    return min(nums)
"#,
        )
        .with_tests(
            "test_stats.py",
            r#"import pytest
from stats import average, maximum, minimum

def test_average():
    assert average([1, 2, 3]) == 2.0
    assert average([10, 20]) == 15.0
    assert average([5]) == 5.0

def test_average_empty():
    with pytest.raises(ValueError):
        average([])

def test_maximum():
    assert maximum([1, 5, 3]) == 5
    assert maximum([-1, -5, -3]) == -1

def test_minimum():
    assert minimum([1, 5, 3]) == 1
    assert minimum([-1, -5, -3]) == -5
"#,
        ),

        // ────────────────────────────────────────────────────────────
        // TIER 4: Real-World Patterns
        // هدف: السيناريوهات الحقيقية الأكثر طلباً
        // ────────────────────────────────────────────────────────────
        BenchCase::new(
            "Real: Python CLI wordcount",
            "Build a Python module 'wordcount.py' with two functions: count_words(text: str) -> dict that counts word frequencies (case-insensitive), and top_words(counts: dict, n: int) -> list of (word, count) tuples sorted by frequency descending.\n\nTests: python -m pytest test_wordcount.py -v",
            "Python",
            4,
            "real: CLI utility",
        )
        .with_tests(
            "test_wordcount.py",
            r#"from wordcount import count_words, top_words

def test_count_words_basic():
    result = count_words("hello world hello")
    assert result["hello"] == 2
    assert result["world"] == 1

def test_count_words_case_insensitive():
    result = count_words("Hello HELLO hello")
    assert result["hello"] == 3

def test_count_words_empty():
    result = count_words("")
    assert result == {}

def test_top_words():
    counts = {"a": 5, "b": 3, "c": 8, "d": 1}
    top = top_words(counts, n=2)
    assert len(top) == 2
    assert top[0] == ("c", 8)
    assert top[1] == ("a", 5)

def test_top_words_less_than_n():
    counts = {"x": 1}
    top = top_words(counts, n=5)
    assert len(top) == 1
"#,
        ),

        BenchCase::new(
            "Real: Go grep-lite",
            "Build a Go file 'grep.go' in package main with an exported function MatchLines(lines []string, pattern string) []string that returns lines matching the regex pattern. Import regexp. Also write a main() that reads from os.Stdin line by line and prints matches for os.Args[1] pattern.\n\nTests: go test -v",
            "Go",
            4,
            "real: CLI with regex",
        )
        .with_tests(
            "grep_test.go",
            r#"package main

import "testing"

func TestMatchLines(t *testing.T) {
    input := []string{"hello world", "foo bar", "hello again", "test"}
    result := MatchLines(input, "hello")
    if len(result) != 2 {
        t.Errorf("Expected 2 matches, got %d", len(result))
    }
}

func TestMatchLinesEmpty(t *testing.T) {
    result := MatchLines([]string{}, "test")
    if len(result) != 0 {
        t.Errorf("Expected 0 matches on empty input, got %d", len(result))
    }
}

func TestMatchLinesNoMatch(t *testing.T) {
    input := []string{"apple", "banana", "cherry"}
    result := MatchLines(input, "^z")
    if len(result) != 0 {
        t.Errorf("Expected 0 matches, got %d", len(result))
    }
}

func TestMatchLinesRegex(t *testing.T) {
    input := []string{"error: file not found", "info: started", "error: timeout"}
    result := MatchLines(input, "^error")
    if len(result) != 2 {
        t.Errorf("Expected 2 error lines, got %d", len(result))
    }
}
"#,
        )
        .with_scaffold("go.mod", "module grep-lite\n\ngo 1.21\n"),

        BenchCase::new(
            "Real: TypeScript validator",
            "Create a TypeScript file 'validator.ts' exporting three functions:\n- isEmail(s: string): boolean — validate standard email formats\n- isUrl(s: string): boolean — true only for http:// or https:// URLs, use try/catch with new URL() and check protocol\n- isStrongPassword(s: string, minLen?: number): boolean — default minLen=8, requires >= 1 uppercase, >= 1 lowercase, >= 1 digit\n\nTests: npx jest validator.test.ts",
            "TypeScript",
            4,
            "real: TS library",
        )
        .with_tests(
            "validator.test.ts",
            r#"import { isEmail, isUrl, isStrongPassword } from './validator';

describe('isEmail', () => {
    test('valid emails', () => {
        expect(isEmail('user@example.com')).toBe(true);
        expect(isEmail('a@b.co')).toBe(true);
    });
    test('invalid emails', () => {
        expect(isEmail('not-an-email')).toBe(false);
        expect(isEmail('@missing.com')).toBe(false);
        expect(isEmail('missing@')).toBe(false);
    });
});

describe('isUrl', () => {
    test('valid urls', () => {
        expect(isUrl('https://example.com')).toBe(true);
        expect(isUrl('http://localhost:3000')).toBe(true);
    });
    test('invalid urls', () => {
        expect(isUrl('not a url')).toBe(false);
        expect(isUrl('ftp://old.com')).toBe(false);
    });
});

describe('isStrongPassword', () => {
    test('strong passwords', () => {
        expect(isStrongPassword('Abcde123', 8)).toBe(true);
        expect(isStrongPassword('MyPass9x', 8)).toBe(true);
        expect(isStrongPassword('MyPass9', 6)).toBe(true);
    });
    test('weak passwords', () => {
        expect(isStrongPassword('short1A', 8)).toBe(false);
        expect(isStrongPassword('alllowercase1', 8)).toBe(false);
        expect(isStrongPassword('ALLUPPERCASE1', 8)).toBe(false);
        expect(isStrongPassword('NoDigitsHere', 8)).toBe(false);
    });
});
"#,
        ),

        BenchCase::new(
            "Real: Rust calculator library",
            "Create a complete Rust library in src/lib.rs for a stack-based calculator. Implement: pub struct Calculator with a stack (Vec<f64>), pub fn push(&mut self, val: f64), pub fn pop(&mut self) -> Result<f64, String>, pub fn add(&mut self) -> Result<f64, String>, pub fn multiply(&mut self) -> Result<f64, String>. Include full #[cfg(test)] module.\n\nTests: cargo test",
            "Rust",
            4,
            "real: Rust library",
        )
        .with_scaffold(
            "Cargo.toml",
            "[package]\nname = \"stack-calc\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
        ),
    ]
}

// ═══════════════════════════════════════════════════════════════════════════
// طباعة الخطة
// ═══════════════════════════════════════════════════════════════════════════

fn print_test_plan(cases: &[BenchCase]) {
    println!("📋 Test Plan ({} cases):\n", cases.len());

    let mut by_tier: std::collections::BTreeMap<u8, Vec<&BenchCase>> =
        std::collections::BTreeMap::new();
    for case in cases {
        by_tier.entry(case.tier).or_default().push(case);
    }

    let tier_names = [
        (1u8, "Compile-First Pipeline"),
        (2u8, "quick_fix (auto, no LLM)"),
        (3u8, "Language Guard + BugFix"),
        (4u8, "Real-World Patterns"),
    ];

    for (tier, name) in &tier_names {
        if let Some(tier_cases) = by_tier.get(tier) {
            println!(
                "  {} Tier {} — {} ({} cases)",
                "●".yellow(),
                tier,
                name.bold(),
                tier_cases.len()
            );
            for c in tier_cases {
                println!(
                    "      {} [{:12}] {}",
                    "→".dimmed(),
                    c.lang.blue(),
                    c.tests_feature.magenta()
                );
            }
        }
    }
    println!();
}

// ═══════════════════════════════════════════════════════════════════════════
// طباعة النتائج
// ═══════════════════════════════════════════════════════════════════════════

fn print_results(
    passed: usize,
    total: usize,
    total_repairs: usize,
    healed: usize,
    elapsed: Duration,
    feature_stats: &std::collections::HashMap<&str, (usize, usize)>,
    tier: Option<u8>,
) {
    let pct = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    let avg_r = if passed > 0 {
        total_repairs as f64 / passed as f64
    } else {
        0.0
    };

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║   SEL Agent v8.2.0 — Benchmark Results                       ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║  Tier    : {}",
        tier.map_or("ALL".to_string(), |t| format!("Tier {}", t))
    );
    println!(
        "║  Time    : {}m {}s",
        elapsed.as_secs() / 60,
        elapsed.as_secs() % 60
    );
    println!("║  Result  : {}/{} ({:.1}%)", passed, total, pct);
    println!("║  AvgFix  : {:.1} repairs/success", avg_r);
    if healed > 0 {
        println!("║  Healed  : {} auto-rerecorded ✨", healed);
    }
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Feature Breakdown                                          ║");
    println!("╠══════════════════════════════════════════════════════════════╣");

    let mut sorted: Vec<_> = feature_stats.iter().collect();
    sorted.sort_by_key(|(k, _)| *k);

    for (feature, (p, t)) in &sorted {
        let fpct = if *t > 0 {
            (*p as f64 / *t as f64) * 100.0
        } else {
            0.0
        };
        let filled = (fpct / 10.0) as usize;
        let bar_raw = format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled));
        let bar = if fpct >= 80.0 {
            bar_raw.green().to_string()
        } else if fpct >= 50.0 {
            bar_raw.yellow().to_string()
        } else {
            bar_raw.red().to_string()
        };
        println!("║  {:<38} {} {}/{}", feature, bar, p, t);
    }

    println!("╠══════════════════════════════════════════════════════════════╣");

    let verdict = if pct >= 90.0 {
        format!("  {} STABLE — ready for production", "✅".green())
    } else if pct >= 70.0 {
        format!(
            "  {} FUNCTIONAL — investigate failures before proceeding",
            "⚠️".yellow()
        )
    } else {
        format!(
            "  {} UNSTABLE — fix issues before any new feature",
            "❌".red()
        )
    };

    println!("║  {}  ║", verdict);
    println!("╚══════════════════════════════════════════════════════════════╝\n");
}
