use std::sync::LazyLock;

static RE_ANSI: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\x1b\[[0-9;]*m").expect("RE_ANSI"));

// src/bench_sel.rs — SELBench v1.0
// Internal benchmark covering all v8.4 capabilities
// 10 core cases + 2 system checks

use anyhow::Result;
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

// ─────────────────────────────────────────────────────────────────
// Data Structures
// ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SelBenchKind {
    Core,
    System,
}

#[derive(Debug, Clone)]
pub struct BenchFile {
    pub path: &'static str,
    pub content: &'static str,
}

#[derive(Debug, Clone)]
pub enum BenchExpectation {
    Pass,
    PassWithNote(&'static str),
    MutantKilled,
    ReplayStable,
    RerecordHealed,
}

#[derive(Debug, Clone)]
pub struct SelBenchCase {
    pub id: &'static str,
    pub kind: SelBenchKind,
    pub title: &'static str,
    pub language: &'static str,
    pub category: &'static str,
    pub goal: &'static str,
    pub files: &'static [BenchFile],
    pub extra_deps: &'static [&'static str],
    pub expectation: BenchExpectation,
}

#[derive(Debug)]
pub struct SelBenchResult {
    pub id: String,
    pub language: String,
    pub category: String,
    pub title: String,
    pub passed: bool,
    pub repairs: u8,
    pub time_secs: f64,

    // telemetry / diagnostics
    pub protocol_auto_injections: u8,
    pub patch_fallbacks: u8,
    pub replan_count: u8,
    pub loop_detections: u8,
}

// ─────────────────────────────────────────────────────────────────
// Main Runner
// ─────────────────────────────────────────────────────────────────

pub async fn run_bench_sel(
    _api_key: &str,
    focus: Option<&str>,
    max_repairs: u8,
    delay: u64,
    record: bool,
    replay: bool,
    rerecord: bool,
) -> Result<()> {
    let all = all_cases();

    let cases: Vec<&SelBenchCase> = all
        .iter()
        .filter(|c| {
            // فلترة System checks — تشغّل فقط عند طلب صريح
            if c.kind == SelBenchKind::System {
                if let Some(ids) = focus {
                    let id_set: std::collections::HashSet<&str> = ids.split(',').collect();
                    return id_set.contains(c.id);
                }
                return false; // System checks محذوفة من default run
            }
            if let Some(ids) = focus {
                let id_set: std::collections::HashSet<&str> = ids.split(',').collect();
                id_set.contains(c.id)
            } else {
                true
            }
        })
        .collect();

    let total = cases.len();
    let mode = if replay && rerecord {
        "REPLAY+RERECORD"
    } else if replay {
        "REPLAY"
    } else if record {
        "RECORD"
    } else {
        "LIVE"
    };

    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║   SELBench v1.0 — SEL Agent Internal Benchmark   ║".cyan()
    );
    println!(
        "{}",
        "╠══════════════════════════════════════════════════╣".cyan()
    );
    println!("║  Cases: {:3}  Mode: {:<22}       ║", total, mode);
    println!(
        "{}",
        "╚══════════════════════════════════════════════════╝".cyan()
    );
    println!();

    let mut results: Vec<SelBenchResult> = Vec::new();

    for (i, case) in cases.iter().enumerate() {
        let ws = build_workspace(case, i)?;
        let start = Instant::now();

        print!(
            "  [{:02}/{:02}] {} {} ",
            i + 1,
            total,
            case.id.bright_cyan(),
            case.title
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();

        // كتابة الملفات
        if let Err(e) = write_files(&ws, case) {
            println!(" → ❌ workspace error: {}", e);
            results.push(SelBenchResult {
                id: case.id.to_string(),
                language: case.language.to_string(),
                category: case.category.to_string(),
                title: case.title.to_string(),
                passed: false,
                repairs: 0,
                time_secs: 0.0,
                protocol_auto_injections: 0,
                patch_fallbacks: 0,
                replan_count: 0,
                loop_detections: 0,
            });
            let _ = fs::remove_dir_all(&ws);
            continue;
        }

        // تهيئة البيئة
        prepare_env(&ws, case).await;

        // trajectory directory
        let traj_dir = std::env::current_dir()
            .unwrap_or_default()
            .join("fixtures")
            .join("trajectories")
            .join(format!("sel_{}", case.id.to_lowercase().replace('-', "_")));

        // تشغيل الوكيل
        let agent_result = run_agent(
            &ws,
            case.goal,
            max_repairs,
            record,
            replay,
            rerecord,
            &traj_dir,
        )
        .await;

        let elapsed = start.elapsed().as_secs_f64();
        let (passed, repairs) = match &agent_result {
            Ok((p, r)) => (*p, *r),
            Err(_) => (false, max_repairs),
        };

        if passed {
            println!(" → ✅ ({:.1}s, {} repairs)", elapsed, repairs);
        } else {
            let reason = agent_result
                .as_ref()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            let short = if reason.len() > 50 {
                &reason[..50]
            } else {
                &reason
            };
            println!(" → ❌ ({:.1}s) {}", elapsed, short.dimmed());
        }

        results.push(SelBenchResult {
            id: case.id.to_string(),
            language: case.language.to_string(),
            category: case.category.to_string(),
            title: case.title.to_string(),
            passed,
            repairs,
            time_secs: elapsed,
            protocol_auto_injections: 0,
            patch_fallbacks: 0,
            replan_count: 0,
            loop_detections: 0,
        });

        let _ = fs::remove_dir_all(&ws);

        if i < total - 1 && delay > 0 {
            print!("     ⏳ {}s cooldown...", delay);
            std::io::Write::flush(&mut std::io::stdout()).ok();
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            print!("\r{}\r", " ".repeat(40));
        }
    }

    print_results(&results, total);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Workspace Helpers
// ─────────────────────────────────────────────────────────────────

fn build_workspace(case: &SelBenchCase, idx: usize) -> Result<PathBuf> {
    let ws = std::env::temp_dir().join(format!(
        "sel-bench-{}-{}",
        case.id.to_lowercase().replace('-', ""),
        idx
    ));
    fs::create_dir_all(&ws)?;
    Ok(ws)
}

fn write_files(ws: &Path, case: &SelBenchCase) -> Result<()> {
    for f in case.files {
        let full = ws.join(f.path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&full, f.content)?;
    }
    Ok(())
}

async fn prepare_env(ws: &Path, case: &SelBenchCase) {
    match case.language {
        "python" => prepare_python(ws, case).await,
        "go" => prepare_go(ws).await,
        "rust" => prepare_rust(ws),
        "typescript" => prepare_typescript(ws).await,
        _ => {}
    }
}

async fn prepare_python(ws: &Path, case: &SelBenchCase) {
    let venv = ws.join("venv");
    if !venv.exists() {
        let _ = tokio::process::Command::new("python3")
            .args(["-m", "venv", "venv"])
            .current_dir(ws)
            .output()
            .await;
    }
    let pip = ws.join("venv/bin/pip3");
    let pip_str = pip.to_str().unwrap_or("pip3");
    let _ = tokio::process::Command::new(pip_str)
        .args(["install", "-q", "pytest"])
        .current_dir(ws)
        .output()
        .await;
    for dep in case.extra_deps {
        let _ = tokio::process::Command::new(pip_str)
            .args(["install", "-q", dep])
            .current_dir(ws)
            .output()
            .await;
    }
}

async fn prepare_go(ws: &Path) {
    if !ws.join("go.mod").exists() {
        let _ = tokio::process::Command::new("go")
            .args(["mod", "init", "sel_bench"])
            .current_dir(ws)
            .output()
            .await;
    }
}

fn prepare_rust(ws: &Path) {
    // Cargo.toml قد يكون موجودًا في الملفات، أو نُنشئ واحدًا بسيطًا
    if !ws.join("Cargo.toml").exists() {
        let toml = r#"[package]
name = "sel_bench"
version = "0.1.0"
edition = "2021"
"#;
        let _ = fs::write(ws.join("Cargo.toml"), toml);
    }
    let src = ws.join("src");
    if !src.exists() {
        let _ = fs::create_dir_all(&src);
    }
}

async fn prepare_typescript(ws: &Path) {
    if !ws.join("package.json").exists() {
        let pkg = r#"{
  "name": "sel-bench",
  "version": "1.0.0",
  "scripts": { "test": "jest" },
  "devDependencies": {
    "jest": "^29",
    "ts-jest": "^29",
    "@types/jest": "^29",
    "typescript": "^5"
  }
}"#;
        let tsconfig = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "strict": true,
    "esModuleInterop": true
  }
}"#;
        let jest = r#"module.exports = { preset: 'ts-jest', testEnvironment: 'node' };"#;
        let _ = fs::write(ws.join("package.json"), pkg);
        let _ = fs::write(ws.join("tsconfig.json"), tsconfig);
        let _ = fs::write(ws.join("jest.config.js"), jest);
    }
    let _ = tokio::process::Command::new("npm")
        .args(["install", "--silent"])
        .current_dir(ws)
        .output()
        .await;
}

// ─────────────────────────────────────────────────────────────────
// Agent Runner
// ─────────────────────────────────────────────────────────────────

async fn run_agent(
    ws: &Path,
    goal: &str,
    max_repairs: u8,
    record: bool,
    replay: bool,
    rerecord: bool,
    traj_dir: &Path,
) -> Result<(bool, u8)> {
    let provider: Box<dyn crate::llm::LLMProvider> = if replay {
        Box::new(crate::llm::replay::ReplayProvider::new(traj_dir))
    } else {
        let live = crate::llm::live::LiveProvider::from_env();
        if record {
            let _ = fs::create_dir_all(traj_dir);
            Box::new(crate::llm::record::RecorderProvider::new(
                Box::new(live),
                traj_dir,
            ))
        } else {
            Box::new(live)
        }
    };

    let mut agent = crate::agent::Agent::new_with_model(
        String::new(),
        String::new(),
        ws.to_path_buf(),
        goal.to_string(),
        max_repairs,
        crate::types::ContextConfig::default(),
        provider,
    );
    agent.ctx.skip_mutation = true;
    agent.bench_mode = true;

    let res = agent.run().await;
    let success = agent.is_success();

    // rerecord: إذا فشل في replay، أعد التسجيل
    if rerecord && !success {
        let live = crate::llm::live::LiveProvider::from_env();
        let _ = fs::create_dir_all(traj_dir);
        let rec = Box::new(crate::llm::record::RecorderProvider::new(
            Box::new(live),
            traj_dir,
        ));
        let _ = fs::remove_dir_all(ws);
        let _ = fs::create_dir_all(ws);
        let mut ag2 = crate::agent::Agent::new_with_model(
            String::new(),
            String::new(),
            ws.to_path_buf(),
            goal.to_string(),
            max_repairs,
            crate::types::ContextConfig::default(),
            rec,
        );
        ag2.ctx.skip_mutation = true;
        ag2.bench_mode = true;
        let _ = ag2.run().await;
        return Ok((ag2.is_success(), ag2.repair_count() as u8));
    }

    match res {
        Ok(_) => Ok((agent.is_success(), agent.repair_count() as u8)),
        Err(e) => Err(e),
    }
}

// ─────────────────────────────────────────────────────────────────
// Results Printer
// ─────────────────────────────────────────────────────────────────

fn print_results(results: &[SelBenchResult], total: usize) {
    let passed = results.iter().filter(|r| r.passed).count();
    let pct = passed as f64 / total as f64 * 100.0;

    // تجميع حسب category
    let categories = [
        "logic",
        "dependency",
        "quickfix",
        "multifile",
        "pre_repair",
        "sanitizer",
        "mutation",
    ];

    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║   SELBench v1.0 — Results                        ║".cyan()
    );
    println!(
        "{}",
        "╠══════════════════════════════════════════════════╣".cyan()
    );
    println!(
        "║  Total:  {:2}/{:2} ({:.1}%){}║",
        passed,
        total,
        pct,
        " ".repeat(26 - format!("{:.1}%", pct).len())
    );
    println!(
        "{}",
        "╠══════════════════════════════════════════════════╣".cyan()
    );
    println!("║  By Category:{}║", " ".repeat(36));

    for cat in &categories {
        let cat_results: Vec<_> = results.iter().filter(|r| r.category == *cat).collect();
        if cat_results.is_empty() {
            continue;
        }
        let cp = cat_results.iter().filter(|r| r.passed).count();
        let ct = cat_results.len();
        let icon = if cp == ct { "✅" } else { "❌" };
        println!(
            "║    {} {:<22} {:2}/{:2}{}║",
            icon,
            format!("{}:", cat),
            cp,
            ct,
            " ".repeat(14 - format!("{:2}/{:2}", cp, ct).len())
        );
    }

    println!(
        "{}",
        "╠══════════════════════════════════════════════════╣".cyan()
    );

    // الحالات الفاشلة
    let failed: Vec<_> = results.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        println!("║  ❌ Failed Cases:{}║", " ".repeat(33));
        for r in &failed {
            let title_short = &r.title[..r.title.len().min(30)];
            println!(
                "║    {} {}{}║",
                r.id.bright_red(),
                title_short,
                " ".repeat(44usize.saturating_sub(r.id.len() + title_short.len() + 1))
            );
        }
        println!(
            "{}",
            "╠══════════════════════════════════════════════════╣".cyan()
        );
    }

    // الحكم النهائي
    let verdict = if pct >= 90.0 {
        "✅ Excellent — all systems nominal".green().to_string()
    } else if pct >= 75.0 {
        "✅ Good — minor issues".green().to_string()
    } else if pct >= 60.0 {
        "⚠️  Acceptable — needs attention".yellow().to_string()
    } else {
        "❌ Critical — core features broken".red().to_string()
    };
    println!(
        "║  {}{}║",
        verdict,
        " ".repeat(49usize.saturating_sub(strip_ansi(&verdict).len()))
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════╝".cyan()
    );
}

fn strip_ansi(s: &str) -> String {
    let re = &*RE_ANSI;
    re.replace_all(s, "").to_string()
}

// ─────────────────────────────────────────────────────────────────
// All Cases
// ─────────────────────────────────────────────────────────────────

pub fn all_cases() -> Vec<SelBenchCase> {
    vec![
        // ═══ SB-01: Logic — Pagination Off-by-One ═══════════════
        SelBenchCase {
            id: "SB-01",
            kind: SelBenchKind::Core,
            title: "Pagination Off-by-One",
            language: "python",
            category: "logic",
            goal: "Fix the bug in calculator.py so the tests pass. \
                   The first page should be page=1, not page=0. \
                   Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "calculator.py",
                    content: "def paginate(items, page, page_size):\n\
                               \x20   start = page * page_size\n\
                               \x20   end = start + page_size\n\
                               \x20   return items[start:end]\n",
                },
                BenchFile {
                    path: "test_calculator.py",
                    content: "from calculator import paginate\n\n\
                               def test_page1():\n\
                               \x20   assert paginate(list(range(10)), 1, 3) == [0,1,2]\n\n\
                               def test_page2():\n\
                               \x20   assert paginate(list(range(10)), 2, 3) == [3,4,5]\n",
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },
        // ═══ SB-02: Logic — Mutable Default Argument ════════════
        SelBenchCase {
            id: "SB-02",
            kind: SelBenchKind::Core,
            title: "Mutable Default Argument",
            language: "python",
            category: "logic",
            goal: "Fix the bug in store.py. \
                   The Store class uses a mutable default argument which causes shared state. \
                   Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "store.py",
                    content: "class Store:\n\
                               \x20   def __init__(self, data={}):\n\
                               \x20       self.data = data\n",
                },
                BenchFile {
                    path: "test_store.py",
                    content: "from store import Store\n\n\
                               def test_isolation():\n\
                               \x20   a = Store()\n\
                               \x20   b = Store()\n\
                               \x20   a.data['x'] = 1\n\
                               \x20   assert b.data == {}\n",
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },
        // ═══ SB-03: Dependency — Missing Python Package ══════════
        SelBenchCase {
            id: "SB-03",
            kind: SelBenchKind::Core,
            title: "Missing Python Dependency",
            language: "python",
            category: "dependency",
            goal: "Fix the project so the tests pass. \
                   The requests library may need to be installed. \
                   Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "main.py",
                    content: "import requests\n\n\
                               def ok():\n\
                               \x20   return requests.__name__\n",
                },
                BenchFile {
                    path: "test_main.py",
                    content: "from main import ok\n\n\
                               def test_ok():\n\
                               \x20   assert ok() == 'requests'\n",
                },
            ],
            extra_deps: &["requests"],
            expectation: BenchExpectation::PassWithNote("pip install requests"),
        },
        // ═══ SB-04: QuickFix — Missing Go Import ═════════════════
        SelBenchCase {
            id: "SB-04",
            kind: SelBenchKind::Core,
            title: "Missing Go Import",
            language: "go",
            category: "quickfix",
            goal: "Fix main.go so the tests pass. \
                   The fmt package is used but not imported. \
                   Do NOT modify the test file. Run go test.",
            files: &[
                BenchFile {
                    path: "main.go",
                    content: "package main\n\n\
                               func Add(a int, b int) int {\n\
                               \x20   fmt.Println(\"run\")\n\
                               \x20   return a + b\n\
                               }\n",
                },
                BenchFile {
                    path: "main_test.go",
                    content: "package main\n\n\
                               import \"testing\"\n\n\
                               func TestAdd(t *testing.T) {\n\
                               \x20   if Add(1, 2) != 3 {\n\
                               \x20       t.Fail()\n\
                               \x20   }\n\
                               }\n",
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::PassWithNote("import fmt"),
        },
        // ═══ SB-05: Dependency — Rust Missing Crate ══════════════
        SelBenchCase {
            id: "SB-05",
            kind: SelBenchKind::Core,
            title: "Rust Missing Crate",
            language: "rust",
            category: "dependency",
            goal: "Fix the Rust project so the tests pass. \
                   The serde_json crate is used but not declared in Cargo.toml. \
                   Add the correct dependency. Run cargo test.",
            files: &[
                BenchFile {
                    path: "src/lib.rs",
                    content: "use serde_json::Value;\n\n\
                               pub fn ok() -> Value {\n\
                               \x20   serde_json::json!({\"a\": 1})\n\
                               }\n",
                },
                BenchFile {
                    path: "Cargo.toml",
                    content: "[package]\n\
                               name = \"sel_bench\"\n\
                               version = \"0.1.0\"\n\
                               edition = \"2021\"\n",
                },
                BenchFile {
                    path: "tests/basic.rs",
                    content: "use sel_bench::ok;\n\n\
                               #[test]\n\
                               fn test_ok() {\n\
                               \x20   assert_eq!(ok()[\"a\"], 1);\n\
                               }\n",
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::PassWithNote("add serde_json to Cargo.toml"),
        },
        // ═══ SB-06: Multifile — Wrong Function Name ══════════════
        SelBenchCase {
            id: "SB-06",
            kind: SelBenchKind::Core,
            title: "Multi-file Wrong Function Name",
            language: "python",
            category: "multifile",
            goal: "Fix the project so the tests pass. \
                   service.py imports parse_user from utils.py but the function \
                   is actually named parse_account. \
                   Fix the import or rename the function. \
                   Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "utils.py",
                    content: "def parse_account():\n\
                               \x20   return 1\n",
                },
                BenchFile {
                    path: "service.py",
                    content: "from utils import parse_user\n\n\
                               def run():\n\
                               \x20   return parse_user()\n",
                },
                BenchFile {
                    path: "test_service.py",
                    content: "from service import run\n\n\
                               def test_run():\n\
                               \x20   assert run() == 1\n",
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },
        // ═══ SB-07: Multifile — Wrong Rust Signature ═════════════
        SelBenchCase {
            id: "SB-07",
            kind: SelBenchKind::Core,
            title: "Rust Wrong Function Signature",
            language: "rust",
            category: "multifile",
            goal: "Fix src/lib.rs so the tests pass. \
                   The login function signature may not match what the tests expect. \
                   Do NOT modify the test file. Run cargo test.",
            files: &[
                BenchFile {
                    path: "Cargo.toml",
                    content: "[package]\n\
                               name = \"sel_bench\"\n\
                               version = \"0.1.0\"\n\
                               edition = \"2021\"\n",
                },
                BenchFile {
                    path: "src/lib.rs",
                    content: "pub fn login(user: String) -> bool {\n\
                               \x20   !user.is_empty()\n\
                               }\n",
                },
                BenchFile {
                    path: "tests/basic.rs",
                    content: "use sel_bench::login;\n\n\
                               #[test]\n\
                               fn test_login_str() {\n\
                               \x20   assert!(login(\"abc\"));\n\
                               }\n\n\
                               #[test]\n\
                               fn test_login_empty() {\n\
                               \x20   assert!(!login(\"\"));\n\
                               }\n",
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },
        // ═══ SB-08: Pre-Repair — NameError Auto Import ═══════════
        SelBenchCase {
            id: "SB-08",
            kind: SelBenchKind::Core,
            title: "NameError Auto Import",
            language: "python",
            category: "pre_repair",
            goal: "Fix main.py so the tests pass. \
                   The datetime module is used but not imported. \
                   Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "main.py",
                    content: "def now():\n\
                               \x20   return datetime.datetime.now()\n",
                },
                BenchFile {
                    path: "test_main.py",
                    content: "from main import now\n\n\
                               def test_now():\n\
                               \x20   assert now() is not None\n",
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::PassWithNote("import datetime"),
        },
        // ═══ SB-09: Sanitizer — Rust Unicode Quote ═══════════════
        SelBenchCase {
            id: "SB-09",
            kind: SelBenchKind::Core,
            title: "Rust Unicode Quote Sanitizer",
            language: "rust",
            category: "sanitizer",
            goal: "Fix src/lib.rs so the tests pass. \
                   There may be unicode quote characters causing compile errors. \
                   Do NOT modify the test file. Run cargo test.",
            files: &[
                BenchFile {
                    path: "Cargo.toml",
                    content: "[package]\n\
                               name = \"sel_bench\"\n\
                               version = \"0.1.0\"\n\
                               edition = \"2021\"\n",
                },
                BenchFile {
                    path: "src/lib.rs",
                    // نستخدم unicode quotes عمداً لاختبار الـ sanitizer
                    content: "pub fn hello() -> &\u{2019}static str {\n\
                               \x20   \u{201C}hello\u{201D}\n\
                               }\n",
                },
                BenchFile {
                    path: "tests/basic.rs",
                    content: "use sel_bench::hello;\n\n\
                               #[test]\n\
                               fn test_hello() {\n\
                               \x20   assert_eq!(hello(), \"hello\");\n\
                               }\n",
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::PassWithNote("unicode quote sanitizer"),
        },
        // ═══ SB-10: Mutation — is_even Resistance ════════════════
        SelBenchCase {
            id: "SB-10",
            kind: SelBenchKind::Core,
            title: "Mutation Resistance",
            language: "python",
            category: "mutation",
            goal: "Fix maths.py if needed and ensure the tests are strong enough \
                   to catch mutations. Run pytest.",
            files: &[
                BenchFile {
                    path: "maths.py",
                    content: "def is_even(x):\n\
                               \x20   return x % 2 == 0\n",
                },
                BenchFile {
                    path: "test_maths.py",
                    content: "from maths import is_even\n\n\
                               def test_even():\n\
                               \x20   assert is_even(2)\n\n\
                               def test_odd():\n\
                               \x20   assert not is_even(3)\n\n\
                               def test_zero():\n\
                               \x20   assert is_even(0)\n\n\
                               def test_negative():\n\
                               \x20   assert is_even(-4)\n\
                               \x20   assert not is_even(-3)\n",
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::MutantKilled,
        },
    ]
}

// ─────────────────────────────────────────────────────────────────
// SELBench v1.1-rc — Wave 1 (RC-01..RC-06)
// ─────────────────────────────────────────────────────────────────

pub fn all_cases_v11_rc() -> Vec<SelBenchCase> {
    vec![
        SelBenchCase {
            id: "RC-01",
            kind: SelBenchKind::Core,
            title: "Python Async Missing Await",
            language: "python",
            category: "async",
            goal: "Fix the async bug in main.py so the tests pass. The coroutine is not being awaited properly. Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "main.py",
                    content: r#"async def fetch_name(db, user_id):
    return db.get(user_id)

async def load_user(db, user_id):
    name = fetch_name(db, user_id)
    return {"name": name}
"#,
                },
                BenchFile {
                    path: "test_main.py",
                    content: r#"import pytest
from main import load_user

class DB:
    async def get(self, user_id):
        return "Alice"

@pytest.mark.asyncio
async def test_load_user():
    result = await load_user(DB(), 1)
    assert result == {"name": "Alice"}
"#,
                },
            ],
            extra_deps: &["pytest-asyncio"],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-02",
            kind: SelBenchKind::Core,
            title: "Python Dataclass Default Factory",
            language: "python",
            category: "datamodel",
            goal: "Fix the dataclass bug in model.py. The field `items: list[str] = []` is a mutable default and causes shared state between instances. Keep `@dataclass`, import `field` from `dataclasses`, and fix it using `field(default_factory=list)`. Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "model.py",
                    content: r#"from dataclasses import dataclass

@dataclass
class Basket:
    items: list[str] = []
"#,
                },
                BenchFile {
                    path: "test_model.py",
                    content: r#"from model import Basket

def test_isolated_lists():
    a = Basket()
    b = Basket()
    a.items.append("x")
    assert b.items == []

def test_set_items():
    c = Basket()
    c.items.append("y")
    assert c.items == ["y"]
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-03",
            kind: SelBenchKind::Core,
            title: "Python Circular Import",
            language: "python",
            category: "multifile",
            goal: "Fix the circular import error so the tests pass. Restructure the imports to break the cycle. Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "a.py",
                    content: r#"from b import get_b

def get_a():
    return 1

def total():
    return get_a() + get_b()
"#,
                },
                BenchFile {
                    path: "b.py",
                    content: r#"from a import get_a

def get_b():
    return get_a()
"#,
                },
                BenchFile {
                    path: "test_total.py",
                    content: r#"from a import total

def test_total():
    assert total() == 2
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-04",
            kind: SelBenchKind::Core,
            title: "Python Pathlib Return Type",
            language: "python",
            category: "typing",
            goal: "Fix the return type bug in path_utils.py. The function should return a Path object not a plain string. Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "path_utils.py",
                    content: r#"from pathlib import Path

def build_path(base: str, name: str):
    return base + "/" + name
"#,
                },
                BenchFile {
                    path: "test_path_utils.py",
                    content: r#"from pathlib import Path
from path_utils import build_path

def test_build_path():
    p = build_path("tmp", "a.txt")
    assert p == Path("tmp") / "a.txt"
    assert isinstance(p, Path)
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-05",
            kind: SelBenchKind::Core,
            title: "Go Nil Map Assignment",
            language: "go",
            category: "runtime",
            goal: "Fix the nil map panic in main.go so the tests pass. The map must be initialized before use. Do NOT modify the test file. Run go test.",
            files: &[
                BenchFile {
                    path: "main.go",
                    content: r#"package main

func Count(words []string) map[string]int {
	var m map[string]int
	for _, w := range words {
		m[w]++
	}
	return m
}
"#,
                },
                BenchFile {
                    path: "main_test.go",
                    content: r#"package main

import "testing"

func TestCount(t *testing.T) {
	got := Count([]string{"a", "b", "a"})
	if got["a"] != 2 || got["b"] != 1 {
		t.Fail()
	}
}

func TestEmpty(t *testing.T) {
	got := Count(nil)
	if len(got) != 0 {
		t.Fail()
	}
}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-06",
            kind: SelBenchKind::Core,
            title: "Go Interface Signature Mismatch",
            language: "go",
            category: "interface",
            goal: "Fix the interface implementation in processor.go so the tests pass. The Run method signature must match the Runner interface. Do NOT modify the test file. Run go test.",
            files: &[
                BenchFile {
                    path: "processor.go",
                    content: r#"package main

type Runner interface {
	Run() string
}

type Job struct{}

func (Job) Run(id int) string {
	return "ok"
}

func Execute(r Runner) string {
	return r.Run()
}
"#,
                },
                BenchFile {
                    path: "processor_test.go",
                    content: r#"package main

import "testing"

func TestExecute(t *testing.T) {
	if Execute(Job{}) != "ok" {
		t.Fail()
	}
}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },
    ]
}

pub fn all_cases_v11_rc_wave2() -> Vec<SelBenchCase> {
    vec![
        SelBenchCase {
            id: "RC-07",
            kind: SelBenchKind::Core,
            title: "Go Unused Import",
            language: "go",
            category: "quickfix",
            goal: "Fix the compile error in normalize.go. There is an unused import that prevents compilation. Do NOT modify the test file. Run go test.",
            files: &[
                BenchFile {
                    path: "normalize.go",
                    content: r#"package main

import (
	"fmt"
	"strings"
)

func Normalize(s string) string {
	return strings.TrimSpace(strings.ToLower(s))
}
"#,
                },
                BenchFile {
                    path: "normalize_test.go",
                    content: r#"package main

import "testing"

func TestNormalize(t *testing.T) {
	if Normalize(" Hi ") != "hi" {
		t.Fail()
	}
}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-08",
            kind: SelBenchKind::Core,
            title: "Go Empty Slice Guard",
            language: "go",
            category: "edge-case",
            goal: "Fix the runtime panic in head.go. The function must handle empty and nil slices. Do NOT modify the test file. Run go test.",
            files: &[
                BenchFile {
                    path: "head.go",
                    content: r#"package main

func Head(xs []int) int {
	return xs[0]
}
"#,
                },
                BenchFile {
                    path: "head_test.go",
                    content: r#"package main

import "testing"

func TestHead(t *testing.T) {
	if Head([]int{5, 6}) != 5 {
		t.Fail()
	}
}

func TestHeadEmpty(t *testing.T) {
	if Head(nil) != 0 {
		t.Fail()
	}
}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-09",
            kind: SelBenchKind::Core,
            title: "Rust Move After Use",
            language: "rust",
            category: "ownership",
            goal: "Fix the ownership/move error in src/lib.rs so the tests pass. The vector is used after being moved. Do NOT modify the test file. Run cargo test.",
            files: &[
                BenchFile {
                    path: "Cargo.toml",
                    content: r#"[package]
name = "sel_bench"
version = "0.1.0"
edition = "2021"
"#,
                },
                BenchFile {
                    path: "src/lib.rs",
                    content: r#"pub fn sum_and_len(nums: Vec<i32>) -> (i32, usize) {
    let sum: i32 = nums.into_iter().sum();
    (sum, nums.len())
}
"#,
                },
                BenchFile {
                    path: "tests/basic.rs",
                    content: r#"use sel_bench::sum_and_len;

#[test]
fn test_sum_and_len() {
    assert_eq!(sum_and_len(vec![1, 2, 3]), (6, 3));
}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-10",
            kind: SelBenchKind::Core,
            title: "Rust Missing Module Export",
            language: "rust",
            category: "module",
            goal: "Fix the module visibility issue so the tests pass. The parse function is not exported from the crate root. Do NOT modify the test file. Run cargo test.",
            files: &[
                BenchFile {
                    path: "Cargo.toml",
                    content: r#"[package]
name = "sel_bench"
version = "0.1.0"
edition = "2021"
"#,
                },
                BenchFile {
                    path: "src/lib.rs",
                    content: r#"mod parser;
"#,
                },
                BenchFile {
                    path: "src/parser.rs",
                    content: r#"pub fn parse() -> i32 {
    1
}
"#,
                },
                BenchFile {
                    path: "tests/basic.rs",
                    content: r#"use sel_bench::parse;

#[test]
fn test_parse() {
    assert_eq!(parse(), 1);
}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-11",
            kind: SelBenchKind::Core,
            title: "Rust Missing Clone Derive",
            language: "rust",
            category: "trait",
            goal: "Fix the trait error in src/lib.rs so the tests pass. The Config struct needs to implement Clone. Do NOT modify the test file. Run cargo test.",
            files: &[
                BenchFile {
                    path: "Cargo.toml",
                    content: r#"[package]
name = "sel_bench"
version = "0.1.0"
edition = "2021"
"#,
                },
                BenchFile {
                    path: "src/lib.rs",
                    content: r#"pub struct Config {
    pub name: String,
}

pub fn duplicate(c: Config) -> (Config, Config) {
    (c.clone(), c)
}
"#,
                },
                BenchFile {
                    path: "tests/basic.rs",
                    content: r#"use sel_bench::{Config, duplicate};

#[test]
fn test_duplicate() {
    let c = Config { name: "x".into() };
    let (a, b) = duplicate(c);
    assert_eq!(a.name, "x");
    assert_eq!(b.name, "x");
}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-12",
            kind: SelBenchKind::Core,
            title: "Rust Iterator &&T Deref",
            language: "rust",
            category: "iterator",
            goal: "Fix the iterator type mismatch in src/lib.rs so the tests pass. The filter closure receives &&i32 not &i32. Do NOT modify the test file. Run cargo test.",
            files: &[
                BenchFile {
                    path: "Cargo.toml",
                    content: r#"[package]
name = "sel_bench"
version = "0.1.0"
edition = "2021"
"#,
                },
                BenchFile {
                    path: "src/lib.rs",
                    content: r#"pub fn sum_even(nums: &[i32]) -> i32 {
    nums.iter().filter(|n| n % 2 == 0).sum()
}
"#,
                },
                BenchFile {
                    path: "tests/basic.rs",
                    content: r#"use sel_bench::sum_even;

#[test]
fn test_sum_even() {
    assert_eq!(sum_even(&[1, 2, 3, 4]), 6);
}

#[test]
fn test_all_odd() {
    assert_eq!(sum_even(&[1, 3, 5]), 0);
}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },
    ]
}

pub fn all_cases_v11_rc_wave3() -> Vec<SelBenchCase> {
    vec![
        SelBenchCase {
            id: "RC-13",
            kind: SelBenchKind::Core,
            title: "Rust Borrow Conflict",
            language: "rust",
            category: "borrow",
            goal: "Fix the borrow checker error in src/lib.rs so the tests pass. Cannot borrow as mutable while borrowed as immutable. Do NOT modify the test file. Run cargo test.",
            files: &[
                BenchFile {
                    path: "Cargo.toml",
                    content: r#"[package]
name = "sel_bench"
version = "0.1.0"
edition = "2021"
"#,
                },
                BenchFile {
                    path: "src/lib.rs",
                    content: r#"pub fn push_if_first_positive(v: &mut Vec<i32>, x: i32) {
    let first = &v[0];
    if *first > 0 {
        v.push(x);
    }
}
"#,
                },
                BenchFile {
                    path: "tests/basic.rs",
                    content: r#"use sel_bench::push_if_first_positive;

#[test]
fn test_push_positive() {
    let mut v = vec![1];
    push_if_first_positive(&mut v, 9);
    assert_eq!(v, vec![1, 9]);
}

#[test]
fn test_no_push_negative() {
    let mut v = vec![-1];
    push_if_first_positive(&mut v, 9);
    assert_eq!(v, vec![-1]);
}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-14",
            kind: SelBenchKind::Core,
            title: "TypeScript Promise.all Missing",
            language: "typescript",
            category: "async",
            goal: "Fix the async bug in src/users.ts so the tests pass. The map returns promises not resolved values. Do NOT modify the test file. Run npm test.",
            files: &[
                BenchFile {
                    path: "src/users.ts",
                    content: r#"export async function loadUsers(db: any, ids: number[]): Promise<any[]> {
  return ids.map(async (id) => db.get(id));
}
"#,
                },
                BenchFile {
                    path: "src/users.test.ts",
                    content: r#"import { loadUsers } from "./users";

describe("loadUsers", () => {
  test("returns resolved users", async () => {
    const db = {
      async get(id: number) {
        return { id };
      },
    };
    const result = await loadUsers(db, [1, 2]);
    expect(result).toEqual([{ id: 1 }, { id: 2 }]);
  });
});
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-15",
            kind: SelBenchKind::Core,
            title: "TypeScript Export Mismatch",
            language: "typescript",
            category: "export",
            goal: "Fix the export mismatch in src/math.ts so the tests pass. The test imports a named export but the file uses default export. Do NOT modify the test file. Run npm test.",
            files: &[
                BenchFile {
                    path: "src/math.ts",
                    content: r#"export default function add(a: number, b: number): number {
  return a + b;
}
"#,
                },
                BenchFile {
                    path: "src/math.test.ts",
                    content: r#"import { add } from "./math";

describe("add", () => {
  test("adds two numbers", () => {
    expect(add(2, 3)).toBe(5);
  });

  test("handles zero", () => {
    expect(add(0, 0)).toBe(0);
  });
});
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-16",
            kind: SelBenchKind::Core,
            title: "TypeScript Nullish vs Falsy",
            language: "typescript",
            category: "logic",
            goal: "Fix the fallback logic in src/profile.ts so the tests pass. Empty string should not fall back to anonymous. Use nullish coalescing not logical OR. Do NOT modify the test file. Run npm test.",
            files: &[
                BenchFile {
                    path: "src/profile.ts",
                    content: r#"export function label(name?: string): string {
  return name || "anonymous";
}
"#,
                },
                BenchFile {
                    path: "src/profile.test.ts",
                    content: r#"import { label } from "./profile";

describe("label", () => {
  test("undefined uses fallback", () => {
    expect(label(undefined)).toBe("anonymous");
  });

  test("empty string is preserved", () => {
    expect(label("")).toBe("");
  });

  test("normal name is returned", () => {
    expect(label("Alice")).toBe("Alice");
  });
});
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-17",
            kind: SelBenchKind::Core,
            title: "Python Config Env Alignment",
            language: "python",
            category: "multifile",
            goal: "Fix the environment variable name mismatch so the tests pass. config.py reads DB_URL but the test sets DATABASE_URL. Align one with the other. Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "config.py",
                    content: r#"import os

def db_url():
    return os.getenv("DB_URL", "sqlite://")
"#,
                },
                BenchFile {
                    path: "service.py",
                    content: r#"from config import db_url

def is_postgres():
    return db_url().startswith("postgres://")
"#,
                },
                BenchFile {
                    path: "test_service.py",
                    content: r#"from service import is_postgres

def test_is_postgres(monkeypatch):
    monkeypatch.setenv("DATABASE_URL", "postgres://localhost/db")
    assert is_postgres() is True

def test_not_postgres(monkeypatch):
    monkeypatch.setenv("DATABASE_URL", "sqlite://")
    assert is_postgres() is False
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-18",
            kind: SelBenchKind::Core,
            title: "Rust Module Rename Re-export",
            language: "rust",
            category: "multifile",
            goal: "Fix the module export mismatch so the tests pass. src/util.rs exports parse_account but lib.rs re-exports parse_user. Fix the function name to match. Do NOT modify the test file. Run cargo test.",
            files: &[
                BenchFile {
                    path: "Cargo.toml",
                    content: r#"[package]
name = "sel_bench"
version = "0.1.0"
edition = "2021"
"#,
                },
                BenchFile {
                    path: "src/lib.rs",
                    content: r#"mod util;
pub use util::parse_user;
"#,
                },
                BenchFile {
                    path: "src/util.rs",
                    content: r#"pub fn parse_account() -> i32 {
    1
}
"#,
                },
                BenchFile {
                    path: "tests/basic.rs",
                    content: r#"use sel_bench::parse_user;

#[test]
fn test_parse_user() {
    assert_eq!(parse_user(), 1);
}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::Pass,
        },

        SelBenchCase {
            id: "RC-S1",
            kind: SelBenchKind::System,
            title: "Replay Determinism",
            language: "python",
            category: "replay",
            goal: "Fix the bug in calculator.py so the tests pass. Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "calculator.py",
                    content: r#"def paginate(items, page, page_size):
    start = page * page_size
    end = start + page_size
    return items[start:end]
"#,
                },
                BenchFile {
                    path: "test_calculator.py",
                    content: r#"from calculator import paginate

def test_page1():
    assert paginate(list(range(10)), 1, 3) == [0,1,2]

def test_page2():
    assert paginate(list(range(10)), 2, 3) == [3,4,5]
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::ReplayStable,
        },

        SelBenchCase {
            id: "RC-S2",
            kind: SelBenchKind::System,
            title: "Auto-rerecord Healing",
            language: "python",
            category: "rerecord",
            goal: "Fix the bug in store.py so the tests pass. Do NOT modify the test file. Run pytest.",
            files: &[
                BenchFile {
                    path: "store.py",
                    content: r#"class Store:
    def __init__(self, data={}):
        self.data = data
"#,
                },
                BenchFile {
                    path: "test_store.py",
                    content: r#"from store import Store

def test_isolation():
    a = Store()
    b = Store()
    a.data["x"] = 1
    assert b.data == {}
"#,
                },
            ],
            extra_deps: &[],
            expectation: BenchExpectation::RerecordHealed,
        },
    ]
}

pub fn all_cases_v11() -> Vec<SelBenchCase> {
    let mut all = all_cases_v11_rc();
    all.extend(all_cases_v11_rc_wave2());
    all.extend(all_cases_v11_rc_wave3());
    all
}

/// Core cases only (excludes System checks)
pub fn core_cases_v11() -> Vec<SelBenchCase> {
    all_cases_v11()
        .into_iter()
        .filter(|c| c.kind == SelBenchKind::Core)
        .collect()
}

/// System checks only
pub fn system_checks_v11() -> Vec<SelBenchCase> {
    all_cases_v11()
        .into_iter()
        .filter(|c| c.kind == SelBenchKind::System)
        .collect()
}

// ─────────────────────────────────────────────────────────────────
// SELBench v1.1-rc Runner
// ─────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run_bench_sel_v11(
    _api_key: &str,
    focus: Option<&str>,
    max_repairs: u8,
    delay: u64,
    include_system: bool,
    record: bool,
    replay: bool,
    rerecord: bool,
) -> anyhow::Result<()> {
    let all = all_cases_v11();

    // فلترة
    let cases: Vec<&SelBenchCase> = all
        .iter()
        .filter(|c| {
            if c.kind == SelBenchKind::System && !include_system {
                if let Some(ids) = focus {
                    let id_set: std::collections::HashSet<&str> = ids.split(',').collect();
                    return id_set.contains(c.id);
                }
                return false;
            }
            if let Some(ids) = focus {
                let id_set: std::collections::HashSet<&str> = ids.split(',').collect();
                return id_set.contains(c.id);
            }
            true
        })
        .collect();

    let core_count = cases
        .iter()
        .filter(|c| c.kind == SelBenchKind::Core)
        .count();
    let sys_count = cases
        .iter()
        .filter(|c| c.kind == SelBenchKind::System)
        .count();
    let total = cases.len();

    let mode = if replay && rerecord {
        "REPLAY+RERECORD"
    } else if replay {
        "REPLAY"
    } else if record {
        "RECORD"
    } else {
        "LIVE"
    };

    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║   SELBench v1.1-rc — Extended Benchmark          ║".cyan()
    );
    println!(
        "{}",
        "╠══════════════════════════════════════════════════╣".cyan()
    );
    println!(
        "║  Core: {:2}  System: {:2}  Mode: {:<16}  ║",
        core_count, sys_count, mode
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════╝".cyan()
    );
    println!();

    let mut results: Vec<SelBenchResult> = Vec::new();

    for (i, case) in cases.iter().enumerate() {
        let ws = build_workspace(case, i)?;
        let start = std::time::Instant::now();

        let kind_tag = if case.kind == SelBenchKind::System {
            "[SYS]".yellow()
        } else {
            "[COR]".green()
        };

        print!(
            "  {} [{:02}/{:02}] {} {} ",
            kind_tag,
            i + 1,
            total,
            case.id.bright_cyan(),
            case.title
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();

        if let Err(e) = write_files(&ws, case) {
            println!(" → ❌ workspace error: {}", e);
            results.push(SelBenchResult {
                id: case.id.to_string(),
                language: case.language.to_string(),
                category: case.category.to_string(),
                title: case.title.to_string(),
                passed: false,
                repairs: 0,
                time_secs: 0.0,
                protocol_auto_injections: 0,
                patch_fallbacks: 0,
                replan_count: 0,
                loop_detections: 0,
            });
            let _ = std::fs::remove_dir_all(&ws);
            continue;
        }

        prepare_env(&ws, case).await;

        let traj_dir = std::env::current_dir()
            .unwrap_or_default()
            .join("fixtures")
            .join("trajectories")
            .join(format!("sel_{}", case.id.to_lowercase().replace('-', "_")));

        let agent_result = run_agent(
            &ws,
            case.goal,
            max_repairs,
            record,
            replay,
            rerecord,
            &traj_dir,
        )
        .await;

        let elapsed = start.elapsed().as_secs_f64();
        let (passed, repairs) = match &agent_result {
            Ok((p, r)) => (*p, *r),
            Err(_) => (false, max_repairs),
        };

        if passed {
            println!(" → ✅ ({:.1}s, {} repairs)", elapsed, repairs);
        } else {
            let reason = agent_result
                .as_ref()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            let short = &reason[..reason.len().min(50)];
            println!(" → ❌ ({:.1}s) {}", elapsed, short.dimmed());
        }

        results.push(SelBenchResult {
            id: case.id.to_string(),
            language: case.language.to_string(),
            category: case.category.to_string(),
            title: case.title.to_string(),
            passed,
            repairs,
            time_secs: elapsed,
            protocol_auto_injections: 0,
            patch_fallbacks: 0,
            replan_count: 0,
            loop_detections: 0,
        });

        let _ = std::fs::remove_dir_all(&ws);

        if i < total - 1 && delay > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        }
    }

    print_results_v11(&results, core_count, sys_count);
    Ok(())
}

fn print_results_v11(results: &[SelBenchResult], core_total: usize, sys_total: usize) {
    let core_passed = results
        .iter()
        .filter(|r| r.category != "replay" && r.category != "rerecord" && r.passed)
        .count();
    let sys_passed = results
        .iter()
        .filter(|r| (r.category == "replay" || r.category == "rerecord") && r.passed)
        .count();

    let total_repairs: u32 = results.iter().map(|r| r.repairs as u32).sum();
    let avg_repairs = total_repairs as f64 / results.len().max(1) as f64;

    let langs = ["python", "go", "rust", "typescript"];

    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║   SELBench v1.1-rc — Results                     ║".cyan()
    );
    println!(
        "{}",
        "╠══════════════════════════════════════════════════╣".cyan()
    );
    println!(
        "║  Core:   {:2}/{:2}{}║",
        core_passed,
        core_total,
        " ".repeat(38 - format!("{:2}/{:2}", core_passed, core_total).len())
    );
    println!(
        "║  System: {:2}/{:2}{}║",
        sys_passed,
        sys_total,
        " ".repeat(38 - format!("{:2}/{:2}", sys_passed, sys_total).len())
    );
    println!(
        "{}",
        "╠══════════════════════════════════════════════════╣".cyan()
    );
    println!("║  By Language:{}║", " ".repeat(36));

    for lang in &langs {
        let lr: Vec<_> = results.iter().filter(|r| r.language == *lang).collect();
        if lr.is_empty() {
            continue;
        }
        let lp = lr.iter().filter(|r| r.passed).count();
        let lt = lr.len();
        let icon = if lp == lt { "✅" } else { "⚠️ " };
        println!(
            "║    {} {:<14} {:2}/{:2}{}║",
            icon,
            format!("{}:", lang),
            lp,
            lt,
            " ".repeat(26 - format!("{:2}/{:2}", lp, lt).len())
        );
    }

    println!(
        "{}",
        "╠══════════════════════════════════════════════════╣".cyan()
    );

    let repair_icon = if avg_repairs <= 0.5 { "✅" } else { "⚠️ " };
    println!(
        "║  {} avg repairs/case: {:.2}{}║",
        repair_icon,
        avg_repairs,
        " ".repeat(27 - format!("{:.2}", avg_repairs).len())
    );

    let failed: Vec<_> = results.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        println!(
            "{}",
            "╠══════════════════════════════════════════════════╣".cyan()
        );
        println!("║  ❌ Failed:{}║", " ".repeat(39));
        for r in &failed {
            let t = &r.title[..r.title.len().min(34)];
            println!(
                "║    {} {}{}║",
                r.id.bright_red(),
                t,
                " ".repeat(44usize.saturating_sub(r.id.len() + t.len() + 1))
            );
        }
    }

    println!(
        "{}",
        "╠══════════════════════════════════════════════════╣".cyan()
    );
    let total = core_passed + sys_passed;
    let total_all = core_total + sys_total;
    let pct = total as f64 / total_all.max(1) as f64 * 100.0;
    let verdict = if pct >= 90.0 {
        "✅ Ready for v1.1 full".green().to_string()
    } else if pct >= 75.0 {
        "⚠️  Needs attention".yellow().to_string()
    } else {
        "❌ Critical issues".red().to_string()
    };
    println!(
        "║  {}{}║",
        verdict,
        " ".repeat(49usize.saturating_sub(strip_ansi(&verdict).len()))
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════╝".cyan()
    );
}
