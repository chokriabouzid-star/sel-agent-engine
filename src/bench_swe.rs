#![allow(clippy::too_many_arguments)]
// src/bench_swe.rs — SEL Agent Mini SWE-Bench v1.1
// 30 اختباراً حقيقياً + trajectory record/replay

use anyhow::Result;
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Safely truncate a string at `max_bytes`, respecting Unicode char boundaries.
/// Avoids panics when slicing strings that contain multi-byte Arabic/emoji chars.
fn truncate_safe(s: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

// ─────────────────────────────────────────────────────────────────
// هياكل البيانات
// ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    fn label(&self) -> &'static str {
        match self {
            Self::Easy   => "🟢",
            Self::Medium => "🟡",
            Self::Hard   => "🔴",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SweCase {
    pub id:           &'static str,
    pub lang:         &'static str,
    pub title:        &'static str,
    pub difficulty:   Difficulty,
    pub source_file:  &'static str,
    pub source_code:  &'static str,
    pub test_file:    &'static str,
    pub test_code:    &'static str,
    pub extra_deps:   &'static [&'static str],
}

#[derive(Debug)]
pub struct SweResult {
    pub case_id:   String,
    pub lang:      String,
    pub title:     String,
    pub passed:    bool,
    pub repairs:   u8,
    pub time_secs: f64,
    pub score:     f64,
}

impl SweResult {
    pub fn compute_score(passed: bool, repairs: u8) -> f64 {
        if !passed { return 0.0; }
        match repairs {
            0     => 1.0,
            1..=2 => 0.8,
            3..=5 => 0.5,
            _     => 0.3,
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// الدالة الرئيسية للتشغيل
// ─────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn run_bench_swe(
    api_key: &str,
    lang_filter: &str,
    max_repairs: u8,
    delay: u64,
    focus: Option<&str>,
    record: bool,
    replay: bool,
    rerecord: bool,
) -> Result<()> {
    let all = all_cases();

    // فلترة اللغة
    let mut cases: Vec<&SweCase> = all
        .iter()
        .filter(|c| lang_filter == "all" || c.lang == lang_filter)
        .collect();

    // فلترة focus
    if let Some(ids_str) = focus {
        let id_set: std::collections::HashSet<&str> = ids_str.split(',').collect();
        cases.retain(|c| id_set.contains(c.id));
    }

    let total = cases.len();

    // ─── Header ───
    println!();
    println!("{}", "╔══════════════════════════════════════════════════╗".cyan());
    println!("{}", "║   SEL Agent — Mini SWE-Bench v1.1                ║".cyan());
    println!("{}", "╠══════════════════════════════════════════════════╣".cyan());
    println!("║  Cases: {:3}  Lang: {:<8}  Mode: {:<11}  ║",
        total,
        lang_filter,
        if replay && rerecord { "REPLAY+RERECORD" } else if replay { "REPLAY" } else if record { "RECORD" } else { "LIVE" });
    println!("{}", "╚══════════════════════════════════════════════════╝".cyan());
    println!();

    let mut results: Vec<SweResult> = Vec::new();

    for (i, case) in cases.iter().enumerate() {
        let ws = build_workspace(case, i)?;
        let start = Instant::now();

        // طباعة الحالة الحالية
        print!("  {} [{:02}/{:02}] {} {}",
            case.difficulty.label(),
            i + 1, total,
            case.id.bright_cyan(),
            case.title);
        std::io::Write::flush(&mut std::io::stdout()).ok();

        // تحضير الملفات
        let prep = prepare_workspace(&ws, case);
        if let Err(e) = prep {
            println!(" → ❌ workspace error: {}", e);
            results.push(SweResult {
                case_id: case.id.to_string(),
                lang: case.lang.to_string(),
                title: case.title.to_string(),
                passed: false, repairs: 0,
                time_secs: 0.0, score: 0.0,
            });
            continue;
        }

        // بناء الـ goal
        let goal = format!(
            "Fix the bug in `{}` so the tests in `{}` pass. \
             Do NOT modify the test file. \
             Run the tests to verify the fix.",
            case.source_file, case.test_file
        );

        // trajectory directory
        let traj_dir = std::env::current_dir()
            .unwrap_or_default()
            .join("fixtures")
            .join("trajectories")
            .join(format!("swe_{}", case.id.to_lowercase().replace('-', "_")));

        // تشغيل الوكيل
        let agent_result = run_agent_for_case(
            &ws, &goal, api_key, max_repairs, case.lang,
            record, replay, rerecord, &traj_dir,
        ).await;

        let elapsed = start.elapsed().as_secs_f64();
        let (passed, repairs) = match &agent_result {
            Ok((p, r)) => (*p, *r),
            Err(_)     => (false, max_repairs),
        };
        let score = SweResult::compute_score(passed, repairs);

        // طباعة النتيجة
        if passed {
            println!(" → ✅ ({:.1}s, {} repairs, {:.0}pts)",
                elapsed, repairs, score * 100.0);
        } else {
            let reason = agent_result.err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            let short = if reason.len() > 40 { &reason[..40] } else { &reason };
            println!(" → ❌ ({:.1}s) {}", elapsed, short.dimmed());
        }

        results.push(SweResult {
            case_id:   case.id.to_string(),
            lang:      case.lang.to_string(),
            title:     case.title.to_string(),
            passed, repairs, time_secs: elapsed, score,
        });

        // تنظيف workspace
        let _ = fs::remove_dir_all(&ws);

        // cooldown بين الحالات
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
// تحضير workspace
// ─────────────────────────────────────────────────────────────────

fn build_workspace(case: &SweCase, idx: usize) -> Result<PathBuf> {
    let ws = std::env::temp_dir().join(format!(
        "sel-swe-{}-{}",
        case.id.to_lowercase().replace('-', ""),
        idx
    ));
    fs::create_dir_all(&ws)?;
    Ok(ws)
}

fn prepare_workspace(ws: &Path, case: &SweCase) -> Result<()> {
    // إنشاء المجلدات الفرعية
    for file in &[case.source_file, case.test_file] {
        if let Some(parent) = Path::new(file).parent() {
            if parent != Path::new("") {
                fs::create_dir_all(ws.join(parent))?;
            }
        }
    }

    // كتابة الملفات
    fs::write(ws.join(case.source_file), case.source_code)?;
    fs::write(ws.join(case.test_file), case.test_code)?;

    // إعداد خاص بالـ Python (تثبيت الحزم الإضافية و venv)
    if case.lang == "python" {
        prepare_python(ws, case)?;
    }

    // إعداد خاص بالـ Rust
    if case.lang == "rust" {
        prepare_rust(ws, case)?;
    }

    // إعداد خاص بالـ TypeScript
    if case.lang == "typescript" {
        prepare_typescript(ws)?;
    }

    Ok(())
}

fn prepare_python(ws: &Path, case: &SweCase) -> Result<()> {
    // إنشاء venv وتثبيت الحزم الإضافية
    let venv_dir = ws.join("venv");
    if !venv_dir.exists() {
        let _ = std::process::Command::new("python3")
            .args(["-m", "venv", "venv"])
            .current_dir(ws)
            .output();
    }
    
    // Always install pytest
    let _ = std::process::Command::new(ws.join("venv/bin/pip3").to_str().unwrap_or("pip3"))
        .args(["install", "-q", "pytest"])
        .current_dir(ws)
        .output();

    for dep in case.extra_deps {
        let _ = std::process::Command::new(ws.join("venv/bin/pip3").to_str().unwrap_or("pip3"))
            .args(["install", "-q", dep])
            .current_dir(ws)
            .output();
    }
    Ok(())
}

fn prepare_rust(ws: &Path, case: &SweCase) -> Result<()> {
    let dev_deps = case.extra_deps
        .iter()
        .map(|d| format!("{} = \"*\"", d))
        .collect::<Vec<_>>()
        .join("\n");

    // تحديد مسار test_file بشكل صحيح
    let test_path = case.test_file.replace('\\', "/");

    let cargo_toml = format!(
        r#"[package]
name = "sel_swe"
version = "0.1.0"
edition = "2021"

[lib]
name = "sel_swe"
path = "src/lib.rs"

[[test]]
name = "swe_test"
path = "{test_path}"

[dev-dependencies]
{dev_deps}
"#
    );

    fs::create_dir_all(ws.join("src"))?;
    fs::write(ws.join("Cargo.toml"), cargo_toml)?;
    Ok(())
}

fn prepare_typescript(ws: &Path) -> Result<()> {
    // package.json مبسط — الـ scaffold engine سيكمله
    let pkg = r#"{
  "name": "sel-swe",
  "version": "1.0.0",
  "scripts": { "test": "jest" },
  "devDependencies": {
    "jest": "^29",
    "ts-jest": "^29",
    "@types/jest": "^29",
    "typescript": "^5"
  }
}
"#;
    let tsconfig = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "strict": true,
    "esModuleInterop": true,
    "outDir": "dist"
  },
  "include": ["src/**/*"]
}
"#;
    let jest_config = r#"module.exports = {
  preset: 'ts-jest',
  testEnvironment: 'node',
};
"#;
    fs::write(ws.join("package.json"), pkg)?;
    fs::write(ws.join("tsconfig.json"), tsconfig)?;
    fs::write(ws.join("jest.config.js"), jest_config)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// تشغيل الوكيل على حالة واحدة
// ─────────────────────────────────────────────────────────────────

async fn run_agent_for_case(
    ws: &Path,
    goal: &str,
    _api_key: &str,
    max_repairs: u8,
    _lang: &str,
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

    // rerecord: إذا فشل، أعد التسجيل مباشرة
    if rerecord && !success {
        let live = crate::llm::live::LiveProvider::from_env();
        let _ = fs::create_dir_all(traj_dir);
        let rec = Box::new(crate::llm::record::RecorderProvider::new(
            Box::new(live), traj_dir,
        ));
        let _ = std::fs::remove_dir_all(ws);
        let _ = std::fs::create_dir_all(ws);
        let mut ag2 = crate::agent::Agent::new_with_model(
            String::new(), String::new(),
            ws.to_path_buf(), goal.to_string(),
            max_repairs, crate::types::ContextConfig::default(), rec,
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
// طباعة النتائج النهائية
// ─────────────────────────────────────────────────────────────────

fn print_results(results: &[SweResult], total: usize) {
    let passed     = results.iter().filter(|r| r.passed).count();
    let total_score: f64 = results.iter().map(|r| r.score).sum();
    let max_score  = total as f64;
    let pct_passed = passed as f64 / total as f64 * 100.0;
    let pct_score  = total_score / max_score * 100.0;

    // ─── تجميع حسب اللغة ───
    let langs = ["python", "go", "rust", "typescript"];

    println!();
    println!("{}", "╔══════════════════════════════════════════════════╗".cyan());
    println!("{}", "║   SEL Mini SWE-Bench — النتائج النهائية          ║".cyan());
    println!("{}", "╠══════════════════════════════════════════════════╣".cyan());
    println!("║  المجموع: {:2}/{:2} ({:.1}%){}║",
        passed, total, pct_passed,
        " ".repeat(24 - format!("{:.1}%", pct_passed).len()));
    println!("║  النقاط:  {:.1}/{:.0} ({:.1}%){}║",
        total_score, max_score, pct_score,
        " ".repeat(22 - format!("{:.1}/{:.0}", total_score, max_score).len()));
    println!("{}", "╠══════════════════════════════════════════════════╣".cyan());
    println!("║  حسب اللغة:{}║", " ".repeat(38));

    for lang in &langs {
        let lang_results: Vec<_> = results.iter().filter(|r| r.lang == *lang).collect();
        if lang_results.is_empty() { continue; }
        let lp = lang_results.iter().filter(|r| r.passed).count();
        let lt = lang_results.len();
        let bar = "█".repeat(lp * 10 / lt.max(1));
        let empty = "░".repeat(10 - bar.chars().count());
        println!("║    {:<12} {}{} {:2}/{:2}{}║",
            format!("{}:", lang),
            bar.green(), empty.dimmed(),
            lp, lt,
            " ".repeat(10 - format!("{:2}/{:2}", lp, lt).len()));
    }

    println!("{}", "╠══════════════════════════════════════════════════╣".cyan());

    // ─── الحالات الفاشلة ───
    let failed: Vec<_> = results.iter().filter(|r| !r.passed).collect();
    if !failed.is_empty() {
        println!("║  ❌ الحالات الفاشلة:{}║", " ".repeat(30));
        for r in &failed {
            let title_snippet = truncate_safe(&r.title, 32);
            println!("║    {} {}{}║",
                r.case_id.bright_red(),
                title_snippet,
                " ".repeat(42usize.saturating_sub(r.case_id.len() + title_snippet.len() + 1)));
        }
        println!("{}", "╠══════════════════════════════════════════════════╣".cyan());
    }

    // ─── الحكم النهائي ───
    let verdict = if pct_score >= 85.0 {
        "✅ ممتاز — جاهز لـ v9.0".green().to_string()
    } else if pct_score >= 75.0 {
        "✅ جيد — polish لـ v9.0".green().to_string()
    } else if pct_score >= 60.0 {
        "⚠️  مقبول — هدف v8.6".yellow().to_string()
    } else if pct_score >= 40.0 {
        "⚠️  ضعيف — يحتاج Repo Map".yellow().to_string()
    } else {
        "❌ يحتاج عمل جوهري".red().to_string()
    };
    println!("║  {}{}║",
        verdict,
        " ".repeat(49usize.saturating_sub(
            strip_ansi(&verdict).len()
        )));
    println!("{}", "╚══════════════════════════════════════════════════╝".cyan());
}

fn strip_ansi(s: &str) -> String {
    // إزالة ANSI codes لحساب الطول الحقيقي
    let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

// ─────────────────────────────────────────────────────────────────
// تعريف الـ 30 حالة — البيانات الحقيقية
// ─────────────────────────────────────────────────────────────────

pub fn all_cases() -> Vec<SweCase> {
    vec![
        // ═══════════════════ PYTHON (10) ═══════════════════════
        SweCase {
            id: "PY-01", lang: "python",
            title: "Off-by-one في pagination",
            difficulty: Difficulty::Easy,
            source_file: "pagination.py",
            source_code: "def paginate(items: list, page: int, page_size: int) -> list:\n    start = page * page_size\n    end = start + page_size\n    return items[start:end]\n",
            test_file: "test_pagination.py",
            test_code: "import pytest\nfrom pagination import paginate\n\ndef test_first_page():\n    assert paginate(list(range(10)), 1, 3) == [0, 1, 2]\n\ndef test_second_page():\n    assert paginate(list(range(10)), 2, 3) == [3, 4, 5]\n\ndef test_last_partial():\n    assert paginate(list(range(10)), 4, 3) == [9]\n",
            extra_deps: &[],
        },
        SweCase {
            id: "PY-02", lang: "python",
            title: "Mutable default argument",
            difficulty: Difficulty::Medium,
            source_file: "cache.py",
            source_code: "class Cache:\n    def __init__(self, store={}):\n        self.store = store\n    def set(self, key: str, value):\n        self.store[key] = value\n    def get(self, key: str):\n        return self.store.get(key)\n",
            test_file: "test_cache.py",
            test_code: "from cache import Cache\n\ndef test_no_shared_state():\n    c1, c2 = Cache(), Cache()\n    c1.set('x', 1)\n    assert c2.get('x') is None\n\ndef test_set_get():\n    c = Cache()\n    c.set('k', 'v')\n    assert c.get('k') == 'v'\n\ndef test_missing():\n    assert Cache().get('?') is None\n",
            extra_deps: &[],
        },
        SweCase {
            id: "PY-03", lang: "python",
            title: "Integer division بدل float",
            difficulty: Difficulty::Easy,
            source_file: "stats.py",
            source_code: "def average(nums):\n    return sum(nums) / len(nums)\n\ndef weighted_average(values, weights):\n    total = sum(v * w for v, w in zip(values, weights))\n    return total // sum(weights)\n",
            test_file: "test_stats.py",
            test_code: "from stats import average, weighted_average\n\ndef test_avg(): assert average([1,2,3]) == 2.0\ndef test_wa_basic(): assert weighted_average([10,20],[1,1]) == 15.0\ndef test_wa_unequal(): assert abs(weighted_average([10,20],[1,3]) - 17.5) < 0.001\ndef test_wa_float(): assert isinstance(weighted_average([7,3],[2,1]), float)\n",
            extra_deps: &[],
        },
        SweCase {
            id: "PY-04", lang: "python",
            title: "Timezone naive datetime comparison",
            difficulty: Difficulty::Medium,
            source_file: "scheduler.py",
            source_code: "from datetime import datetime\n\ndef is_expired(expiry_ts: float) -> bool:\n    expiry = datetime.fromtimestamp(expiry_ts)\n    now = datetime.utcnow()\n    return now > expiry\n",
            test_file: "test_scheduler.py",
            test_code: "import time\nfrom scheduler import is_expired\n\ndef test_past(): assert is_expired(time.time() - 3600) is True\ndef test_future(): assert is_expired(time.time() + 3600) is False\ndef test_recent(): assert is_expired(time.time() - 1) is True\n",
            extra_deps: &[],
        },
        SweCase {
            id: "PY-05", lang: "python",
            title: "Bare except يبتلع KeyboardInterrupt",
            difficulty: Difficulty::Medium,
            source_file: "fetcher.py",
            source_code: "import requests\n\ndef fetch_json(url: str) -> dict:\n    try:\n        r = requests.get(url, timeout=5)\n        r.raise_for_status()\n        return r.json()\n    except:\n        return {}\n",
            test_file: "test_fetcher.py",
            test_code: "import pytest\nfrom unittest.mock import patch, MagicMock\nimport requests\nfrom fetcher import fetch_json\n\ndef test_success():\n    m = MagicMock()\n    m.json.return_value = {'k': 'v'}\n    m.raise_for_status.return_value = None\n    with patch('requests.get', return_value=m):\n        assert fetch_json('http://x') == {'k': 'v'}\n\ndef test_http_error():\n    m = MagicMock()\n    m.raise_for_status.side_effect = requests.exceptions.HTTPError()\n    with patch('requests.get', return_value=m):\n        assert fetch_json('http://x') == {}\n\ndef test_keyboard_interrupt():\n    with patch('requests.get', side_effect=KeyboardInterrupt):\n        with pytest.raises(KeyboardInterrupt):\n            fetch_json('http://x')\n",
            extra_deps: &["requests", "pytest"],
        },
        SweCase {
            id: "PY-06", lang: "python",
            title: "Unsafe byte truncation لـ Unicode",
            difficulty: Difficulty::Medium,
            source_file: "truncate.py",
            source_code: "def truncate(text: str, max_bytes: int) -> str:\n    encoded = text.encode('utf-8')\n    if len(encoded) <= max_bytes:\n        return text\n    return encoded[:max_bytes].decode('utf-8')\n",
            test_file: "test_truncate.py",
            test_code: "from truncate import truncate\n\ndef test_ascii_safe(): assert truncate('hello', 10) == 'hello'\ndef test_ascii_cut(): assert truncate('hello world', 5) == 'hello'\ndef test_arabic_valid():\n    r = truncate('مرحبا', 6)\n    assert isinstance(r, str); r.encode('utf-8')\ndef test_emoji_safe():\n    r = truncate('Hi 👋 there', 5)\n    assert isinstance(r, str); r.encode('utf-8')\n",
            extra_deps: &[],
        },
        SweCase {
            id: "PY-07", lang: "python",
            title: "Thread-unsafe counter",
            difficulty: Difficulty::Medium,
            source_file: "counter.py",
            source_code: "import threading\n\nclass Counter:\n    def __init__(self):\n        self.value = 0\n    def increment(self):\n        current = self.value\n        self.value = current + 1\n    def get(self):\n        return self.value\n",
            test_file: "test_counter.py",
            test_code: "import threading\nfrom counter import Counter\n\ndef test_single():\n    c = Counter()\n    for _ in range(100): c.increment()\n    assert c.get() == 100\n\ndef test_concurrent():\n    c = Counter()\n    ts = [threading.Thread(target=lambda: [c.increment() for _ in range(100)]) for _ in range(10)]\n    for t in ts: t.start()\n    for t in ts: t.join()\n    assert c.get() == 1000\n",
            extra_deps: &["pytest"],
        },
        SweCase {
            id: "PY-08", lang: "python",
            title: "Retry لا يُعيد original exception",
            difficulty: Difficulty::Medium,
            source_file: "retry.py",
            source_code: "import time\n\ndef retry(func, max_attempts=3, delay=0.1):\n    attempts = 0\n    while attempts < max_attempts:\n        try:\n            return func()\n        except Exception:\n            attempts += 1\n            time.sleep(delay)\n    raise RuntimeError(f'Failed after {max_attempts} attempts')\n",
            test_file: "test_retry.py",
            test_code: "import pytest\nfrom retry import retry\n\ndef test_success(): assert retry(lambda: 42) == 42\n\ndef test_flaky():\n    calls = [0]\n    def f():\n        calls[0] += 1\n        if calls[0] < 3: raise ValueError('not yet')\n        return 'ok'\n    assert retry(f, max_attempts=3, delay=0) == 'ok'\n\ndef test_original_exception():\n    with pytest.raises(ValueError, match='specific error'):\n        retry(lambda: (_ for _ in ()).throw(ValueError('specific error')), max_attempts=2, delay=0)\n",
            extra_deps: &[],
        },
        SweCase {
            id: "PY-09", lang: "python",
            title: "Binary search off-by-one bound",
            difficulty: Difficulty::Easy,
            source_file: "search.py",
            source_code: "def binary_search(arr: list, target: int) -> int:\n    left, right = 0, len(arr)\n    while left <= right:\n        mid = (left + right) // 2\n        if arr[mid] == target: return mid\n        elif arr[mid] < target: left = mid + 1\n        else: right = mid - 1\n    return -1\n",
            test_file: "test_search.py",
            test_code: "from search import binary_search\n\ndef test_mid(): assert binary_search([1,3,5,7,9], 5) == 2\ndef test_first(): assert binary_search([1,3,5,7,9], 1) == 0\ndef test_last(): assert binary_search([1,3,5,7,9], 9) == 4\ndef test_miss(): assert binary_search([1,3,5,7,9], 4) == -1\ndef test_empty(): assert binary_search([], 5) == -1\n",
            extra_deps: &[],
        },
        SweCase {
            id: "PY-10", lang: "python",
            title: "CSV parser يكسر quoted fields",
            difficulty: Difficulty::Hard,
            source_file: "csv_parser.py",
            source_code: "def parse_csv_line(line: str) -> list:\n    return line.strip().split(',')\n",
            test_file: "test_csv_parser.py",
            test_code: "from csv_parser import parse_csv_line\n\ndef test_simple(): assert parse_csv_line('a,b,c') == ['a','b','c']\ndef test_quoted(): assert parse_csv_line('name,\"Smith, John\",age') == ['name','Smith, John','age']\ndef test_empty_field(): assert parse_csv_line('a,,c') == ['a','','c']\n",
            extra_deps: &[],
        },

        // ═══════════════════ GO (7) ════════════════════════════
        SweCase {
            id: "GO-01", lang: "go",
            title: "Nil pointer في GetEmail",
            difficulty: Difficulty::Easy,
            source_file: "user.go",
            source_code: "package main\n\ntype User struct {\n    Name  string\n    Email *string\n}\n\nfunc GetEmail(u *User) string {\n    return *u.Email\n}\n",
            test_file: "user_test.go",
            test_code: "package main\n\nimport \"testing\"\n\nfunc TestGetEmailValid(t *testing.T) {\n    e := \"a@b.com\"; u := &User{Email: &e}\n    if GetEmail(u) != \"a@b.com\" { t.Fail() }\n}\nfunc TestGetEmailNilEmail(t *testing.T) {\n    if GetEmail(&User{}) != \"\" { t.Error(\"expected empty\") }\n}\nfunc TestGetEmailNilUser(t *testing.T) {\n    if GetEmail(nil) != \"\" { t.Error(\"expected empty\") }\n}\n",
            extra_deps: &[],
        },
        SweCase {
            id: "GO-02", lang: "go",
            title: "IsAdult يجب >= 18",
            difficulty: Difficulty::Easy,
            source_file: "age.go",
            source_code: "package main\n\nimport \"time\"\n\nfunc CalculateAge(birthYear int) int { return time.Now().Year() - birthYear }\n\nfunc IsAdult(birthYear int) bool { return CalculateAge(birthYear) > 18 }\n",
            test_file: "age_test.go",
            test_code: "package main\n\nimport (\"testing\"; \"time\")\n\nfunc TestAge(t *testing.T) {\n    if CalculateAge(time.Now().Year()-25) != 25 { t.Fail() }\n}\nfunc TestExactly18(t *testing.T) {\n    if !IsAdult(time.Now().Year()-18) { t.Error(\"18 should be adult\") }\n}\nfunc TestUnder18(t *testing.T) {\n    if IsAdult(time.Now().Year()-17) { t.Error(\"17 should not be adult\") }\n}\n",
            extra_deps: &[],
        },
        SweCase {
            id: "GO-03", lang: "go",
            title: "Race condition بدون WaitGroup",
            difficulty: Difficulty::Hard,
            source_file: "worker.go",
            source_code: "package main\n\nfunc ProcessItems(items []string) []string {\n    results := make([]string, len(items))\n    for i, item := range items {\n        go func(i int, item string) {\n            results[i] = \"processed:\" + item\n        }(i, item)\n    }\n    return results\n}\n",
            test_file: "worker_test.go",
            test_code: "package main\n\nimport (\"strings\"; \"testing\")\n\nfunc TestProcess(t *testing.T) {\n    r := ProcessItems([]string{\"a\",\"b\",\"c\",\"d\",\"e\"})\n    if len(r) != 5 { t.Fatalf(\"got %d\", len(r)) }\n    for _, v := range r {\n        if !strings.HasPrefix(v, \"processed:\") { t.Errorf(\"bad: %q\", v) }\n    }\n}\nfunc TestEmpty(t *testing.T) {\n    if len(ProcessItems(nil)) != 0 { t.Fail() }\n}\n",
            extra_deps: &[],
        },
        SweCase {
            id: "GO-04", lang: "go",
            title: "Error لا يضم الخطأ الأصلي",
            difficulty: Difficulty::Easy,
            source_file: "parser.go",
            source_code: "package main\n\nimport (\"fmt\"; \"strconv\")\n\nfunc ParsePort(s string) (int, error) {\n    n, err := strconv.Atoi(s)\n    if err != nil { return 0, fmt.Errorf(\"invalid port\") }\n    if n < 1 || n > 65535 { return 0, fmt.Errorf(\"out of range\") }\n    return n, nil\n}\n",
            test_file: "parser_test.go",
            test_code: "package main\n\nimport (\"errors\"; \"strconv\"; \"testing\")\n\nfunc TestValid(t *testing.T) {\n    if p, e := ParsePort(\"8080\"); e != nil || p != 8080 { t.Fail() }\n}\nfunc TestWraps(t *testing.T) {\n    _, err := ParsePort(\"abc\")\n    var ne *strconv.NumError\n    if !errors.As(err, &ne) { t.Error(\"should wrap NumError\") }\n}\nfunc TestRange(t *testing.T) {\n    if _, e := ParsePort(\"99999\"); e == nil { t.Fail() }\n}\n",
            extra_deps: &[],
        },
        SweCase {
            id: "GO-05", lang: "go",
            title: "Concurrent map read/write",
            difficulty: Difficulty::Medium,
            source_file: "cache.go",
            source_code: "package main\n\ntype SimpleCache struct{ data map[string]string }\n\nfunc NewSimpleCache() *SimpleCache { return &SimpleCache{data: make(map[string]string)} }\nfunc (c *SimpleCache) Set(k, v string) { c.data[k] = v }\nfunc (c *SimpleCache) Get(k string) (string, bool) { v, ok := c.data[k]; return v, ok }\n",
            test_file: "cache_test.go",
            test_code: "package main\n\nimport (\"fmt\"; \"sync\"; \"testing\")\n\nfunc TestBasic(t *testing.T) {\n    c := NewSimpleCache(); c.Set(\"k\",\"v\")\n    if v, ok := c.Get(\"k\"); !ok || v != \"v\" { t.Fail() }\n}\nfunc TestMiss(t *testing.T) {\n    if _, ok := NewSimpleCache().Get(\"x\"); ok { t.Fail() }\n}\nfunc TestConcurrent(t *testing.T) {\n    c := NewSimpleCache()\n    var wg sync.WaitGroup\n    for i := 0; i < 100; i++ {\n        wg.Add(1)\n        go func(i int) { defer wg.Done(); k := fmt.Sprintf(\"k%d\",i); c.Set(k,k); c.Get(k) }(i)\n    }\n    wg.Wait()\n}\n",
            extra_deps: &[],
        },
        SweCase {
            id: "GO-06", lang: "go",
            title: "Slice append يُغيّر backing array",
            difficulty: Difficulty::Medium,
            source_file: "filter.go",
            source_code: "package main\n\nfunc FilterPositive(nums []int) []int {\n    result := nums[:0]\n    for _, n := range nums {\n        if n > 0 { result = append(result, n) }\n    }\n    return result\n}\n",
            test_file: "filter_test.go",
            test_code: "package main\n\nimport (\"reflect\"; \"testing\")\n\nfunc TestFilter(t *testing.T) {\n    if !reflect.DeepEqual(FilterPositive([]int{-1,2,-3,4,-5}), []int{2,4}) { t.Fail() }\n}\nfunc TestNoMutate(t *testing.T) {\n    in := []int{1,-2,3}; orig := []int{1,-2,3}\n    FilterPositive(in)\n    if !reflect.DeepEqual(in, orig) { t.Error(\"mutated\") }\n}\n",
            extra_deps: &[],
        },
        SweCase {
            id: "GO-07", lang: "go",
            title: "CountWords key mismatch بعد ToLower",
            difficulty: Difficulty::Easy,
            source_file: "words.go",
            source_code: "package main\n\nimport \"strings\"\n\nfunc CountWords(text string) map[string]int {\n    c := make(map[string]int)\n    for _, w := range strings.Fields(text) {\n        c[strings.ToLower(w)] = c[w] + 1\n    }\n    return c\n}\n",
            test_file: "words_test.go",
            test_code: "package main\n\nimport \"testing\"\n\nfunc TestCount(t *testing.T) {\n    c := CountWords(\"The the THE cat cat\")\n    if c[\"the\"] != 3 { t.Errorf(\"the=%d\",c[\"the\"]) }\n    if c[\"cat\"] != 2 { t.Errorf(\"cat=%d\",c[\"cat\"]) }\n}\n",
            extra_deps: &[],
        },

        // ═══════════════════ RUST (7) ══════════════════════════
        SweCase {
            id: "RS-01", lang: "rust",
            title: "u32 overflow في factorial",
            difficulty: Difficulty::Easy,
            source_file: "src/lib.rs",
            source_code: "pub fn factorial(n: u64) -> u64 {\n    if n == 0 { return 1; }\n    n * factorial(n - 1)\n}\n",
            test_file: "tests/integration.rs",
            test_code: "use sel_swe::factorial;\n#[test] fn zero() { assert_eq!(factorial(0), 1); }\n#[test] fn five() { assert_eq!(factorial(5), 120); }\n#[test] fn twelve() { assert_eq!(factorial(12), 479001600); }\n#[test] fn thirteen() { assert_eq!(factorial(13), 6227020800); }\n",
            extra_deps: &[],
        },
        SweCase {
            id: "RS-02", lang: "rust",
            title: "Parse error لا يكشف القيمة",
            difficulty: Difficulty::Easy,
            source_file: "src/lib.rs",
            source_code: "use std::fs;\n\npub fn read_number(path: &str) -> Result<i64, String> {\n    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;\n    content.trim().parse::<i64>().map_err(|_| \"parse error\".to_string())\n}\n",
            test_file: "tests/integration.rs",
            test_code: "use sel_swe::read_number;\nuse std::io::Write;\n#[test] fn valid() {\n    let mut f = tempfile::NamedTempFile::new().unwrap();\n    write!(f, \"42\").unwrap();\n    assert_eq!(read_number(f.path().to_str().unwrap()), Ok(42));\n}\n#[test] fn error_descriptive() {\n    let mut f = tempfile::NamedTempFile::new().unwrap();\n    write!(f, \"bad_value\").unwrap();\n    let e = read_number(f.path().to_str().unwrap()).unwrap_err();\n    assert!(e.contains(\"bad_value\") || e.contains(\"invalid\"), \"got: {}\", e);\n}\n",
            extra_deps: &["tempfile"],
        },
        SweCase {
            id: "RS-03", lang: "rust",
            title: "Off-by-one في sliding window",
            difficulty: Difficulty::Medium,
            source_file: "src/lib.rs",
            source_code: "pub fn max_sum_subarray(arr: &[i32], k: usize) -> Option<i32> {\n    if arr.len() < k { return None; }\n    let mut s: i32 = arr[..k].iter().sum();\n    let mut max = s;\n    for i in k..arr.len() {\n        s += arr[i] - arr[i - k - 1];\n        max = max.max(s);\n    }\n    Some(max)\n}\n",
            test_file: "tests/integration.rs",
            test_code: "use sel_swe::max_sum_subarray;\n#[test] fn basic() { assert_eq!(max_sum_subarray(&[2,1,5,1,3,2],3), Some(9)); }\n#[test] fn same()  { assert_eq!(max_sum_subarray(&[3,3,3,3],2), Some(6)); }\n#[test] fn short() { assert_eq!(max_sum_subarray(&[1,2],3), None); }\n#[test] fn single(){ assert_eq!(max_sum_subarray(&[4,2,1],3), Some(7)); }\n",
            extra_deps: &[],
        },
        SweCase {
            id: "RS-04", lang: "rust",
            title: "deduplicate لا يحفظ الترتيب",
            difficulty: Difficulty::Medium,
            source_file: "src/lib.rs",
            source_code: "use std::collections::HashSet;\npub fn deduplicate(items: Vec<String>) -> Vec<String> {\n    let mut seen = HashSet::new();\n    items.into_iter().filter(|i| seen.insert(i.clone())).collect()\n}\npub fn most_frequent<'a>(items: &[&'a str]) -> Option<&'a str> {\n    let mut c = std::collections::HashMap::new();\n    for &i in items { *c.entry(i).or_insert(0) += 1; }\n    c.into_iter().max_by_key(|&(_,v)| v).map(|(k,_)| k)\n}\n",
            test_file: "tests/integration.rs",
            test_code: "use sel_swe::{deduplicate, most_frequent};\nfn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }\n#[test] fn dedup_count() { assert_eq!(deduplicate(s(&[\"a\",\"b\",\"a\",\"c\"])).len(), 3); }\n#[test] fn dedup_order() { assert_eq!(deduplicate(s(&[\"c\",\"a\",\"b\",\"a\"]))[0], \"c\"); }\n#[test] fn freq() { assert_eq!(most_frequent(&[\"a\",\"b\",\"a\",\"c\",\"a\",\"b\"]), Some(\"a\")); }\n#[test] fn freq_empty() { assert_eq!(most_frequent(&[]), None); }\n",
            extra_deps: &[],
        },
        SweCase {
            id: "RS-05", lang: "rust",
            title: "Health underflow/overflow",
            difficulty: Difficulty::Easy,
            source_file: "src/lib.rs",
            source_code: "pub struct Health { pub current: u32, pub max: u32 }\nimpl Health {\n    pub fn new(max: u32) -> Self { Self { current: max, max } }\n    pub fn take_damage(&mut self, n: u32) { self.current -= n; }\n    pub fn heal(&mut self, n: u32) { self.current += n; }\n    pub fn is_alive(&self) -> bool { self.current > 0 }\n}\n",
            test_file: "tests/integration.rs",
            test_code: "use sel_swe::Health;\n#[test] fn damage() { let mut h=Health::new(100); h.take_damage(30); assert_eq!(h.current,70); }\n#[test] fn overkill() { let mut h=Health::new(10); h.take_damage(100); assert_eq!(h.current,0); assert!(!h.is_alive()); }\n#[test] fn overheal() { let mut h=Health::new(100); h.take_damage(50); h.heal(200); assert_eq!(h.current,100); }\n",
            extra_deps: &[],
        },
        SweCase {
            id: "RS-06", lang: "rust",
            title: "truncate_words يضيف '...' دائماً",
            difficulty: Difficulty::Medium,
            source_file: "src/lib.rs",
            source_code: "pub fn truncate_words(s: &str, max: usize) -> String {\n    s.split_whitespace().take(max).collect::<Vec<_>>().join(\" \") + \"...\"\n}\n",
            test_file: "tests/integration.rs",
            test_code: "use sel_swe::truncate_words;\n#[test] fn long() { assert_eq!(truncate_words(\"one two three four five\",3), \"one two three...\"); }\n#[test] fn short_no_ellipsis() { assert_eq!(truncate_words(\"hello world\",5), \"hello world\"); }\n#[test] fn exact_no_ellipsis() { assert_eq!(truncate_words(\"a b c\",3), \"a b c\"); }\n",
            extra_deps: &[],
        },
        SweCase {
            id: "RS-07", lang: "rust",
            title: "Query string non-deterministic",
            difficulty: Difficulty::Medium,
            source_file: "src/lib.rs",
            source_code: "use std::collections::HashMap;\npub fn build_query(params: &HashMap<String,String>) -> String {\n    params.iter().map(|(k,v)| format!(\"{}={}\",k,v)).collect::<Vec<_>>().join(\"&\")\n}\n",
            test_file: "tests/integration.rs",
            test_code: "use sel_swe::build_query;\nuse std::collections::HashMap;\n#[test] fn single() { let mut p=HashMap::new(); p.insert(\"k\".into(),\"v\".into()); assert_eq!(build_query(&p),\"k=v\"); }\n#[test] fn sorted() { let mut p=HashMap::new(); p.insert(\"b\".into(),\"2\".into()); p.insert(\"a\".into(),\"1\".into()); assert_eq!(build_query(&p),\"a=1&b=2\"); }\n#[test] fn empty() { assert_eq!(build_query(&HashMap::new()),\"\"); }\n",
            extra_deps: &[],
        },

        // ═══════════════════ TYPESCRIPT (6) ═══════════════════
        SweCase {
            id: "TS-01", lang: "typescript",
            title: "Promise not awaited في saveUser",
            difficulty: Difficulty::Easy,
            source_file: "src/database.ts",
            source_code: "export async function saveUser(db: any, user: {name: string}): Promise<string> {\n    const id = Math.random().toString(36).slice(2);\n    db.insert(id, user);\n    return id;\n}\nexport async function getUser(db: any, id: string) { return db.find(id); }\n",
            test_file: "src/database.test.ts",
            test_code: "import {saveUser, getUser} from './database';\nconst mkdb = () => ({ store: {} as any, insert: async (id:string,u:any)=>{mkdb._s=mkdb._s||{}}, find: async (id:string)=>null });\ndescribe('db', () => {\n    test('returns id', async () => {\n        const db={store:{} as any, insert:async(id:string,u:any)=>{db.store[id]=u;}, find:async(id:string)=>db.store[id]??null};\n        expect(typeof await saveUser(db,{name:'A'})).toBe('string');\n    });\n    test('retrieves saved', async () => {\n        const db={store:{} as any, insert:async(id:string,u:any)=>{db.store[id]=u;}, find:async(id:string)=>db.store[id]??null};\n        const id = await saveUser(db,{name:'B'});\n        expect(await getUser(db,id)).toEqual({name:'B'});\n    });\n});\n",
            extra_deps: &[],
        },
        SweCase {
            id: "TS-02", lang: "typescript",
            title: "Optional chaining مفقود",
            difficulty: Difficulty::Easy,
            source_file: "src/transform.ts",
            source_code: "interface R { data?: {user?: {profile?: {avatar?: string}}} }\nexport function getAvatarUrl(r: R): string {\n    return r.data.user.profile.avatar || '/default.png';\n}\n",
            test_file: "src/transform.test.ts",
            test_code: "import {getAvatarUrl} from './transform';\ndescribe('avatar', () => {\n    test('full', () => expect(getAvatarUrl({data:{user:{profile:{avatar:'https://x.com/a.jpg'}}}})).toBe('https://x.com/a.jpg'));\n    test('no avatar', () => expect(getAvatarUrl({data:{user:{profile:{}}}})).toBe('/default.png'));\n    test('empty', () => expect(getAvatarUrl({})).toBe('/default.png'));\n});\n",
            extra_deps: &[],
        },
        SweCase {
            id: "TS-03", lang: "typescript",
            title: "Array mutation بدل immutable",
            difficulty: Difficulty::Medium,
            source_file: "src/store.ts",
            source_code: "export type Item = {id: number; value: string};\nexport function addItem(items: Item[], item: Item): Item[] { items.push(item); return items; }\nexport function removeItem(items: Item[], id: number): Item[] { items.splice(items.findIndex(i=>i.id===id),1); return items; }\nexport function updateItem(items: Item[], id: number, v: string): Item[] { const i=items.find(i=>i.id===id); if(i) i.value=v; return items; }\n",
            test_file: "src/store.test.ts",
            test_code: "import {addItem,removeItem,updateItem} from './store';\ndescribe('immutability', () => {\n    test('add no mutate', () => { const o=[{id:1,value:'a'}]; const c=[...o]; addItem(o,{id:2,value:'b'}); expect(o).toEqual(c); });\n    test('add returns new', () => { expect(addItem([{id:1,value:'a'}],{id:2,value:'b'})).toHaveLength(2); });\n    test('remove no mutate', () => { const o=[{id:1,value:'a'},{id:2,value:'b'}]; const c=[...o]; removeItem(o,1); expect(o).toEqual(c); });\n    test('update no mutate obj', () => { const o=[{id:1,value:'old'}]; const r=updateItem(o,1,'new'); expect(o[0].value).toBe('old'); expect(r[0].value).toBe('new'); });\n});\n",
            extra_deps: &[],
        },
        SweCase {
            id: "TS-04", lang: "typescript",
            title: "Date month 0-indexed + zero-padding",
            difficulty: Difficulty::Easy,
            source_file: "src/dateUtils.ts",
            source_code: "export function formatDate(d: Date): string {\n    return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;\n}\nexport function daysBetween(a: Date, b: Date): number {\n    return Math.floor((b.getTime()-a.getTime())/(86400000));\n}\n",
            test_file: "src/dateUtils.test.ts",
            test_code: "import {formatDate, daysBetween} from './dateUtils';\ndescribe('dates', () => {\n    test('jan', () => expect(formatDate(new Date(2024,0,5))).toBe('2024-01-05'));\n    test('dec', () => expect(formatDate(new Date(2024,11,25))).toBe('2024-12-25'));\n    test('double digit', () => expect(formatDate(new Date(2024,5,15))).toBe('2024-06-15'));\n    test('daysBetween', () => expect(daysBetween(new Date(2024,0,1),new Date(2024,0,8))).toBe(7));\n});\n",
            extra_deps: &[],
        },
        SweCase {
            id: "TS-05", lang: "typescript",
            title: "EventBus.off لا يُزيل handler",
            difficulty: Difficulty::Medium,
            source_file: "src/eventBus.ts",
            source_code: "type H=(d:any)=>void;\nexport class EventBus {\n    private h: Map<string,H[]>=new Map();\n    on(e:string,h:H){ if(!this.h.has(e))this.h.set(e,[]); this.h.get(e)!.push(h); }\n    off(e:string,h:H){ const l=this.h.get(e)||[]; this.h.set(e,l); }\n    emit(e:string,d:any){ (this.h.get(e)||[]).forEach(h=>h(d)); }\n}\n",
            test_file: "src/eventBus.test.ts",
            test_code: "import {EventBus} from './eventBus';\ndescribe('EventBus', () => {\n    test('on/emit', () => { const b=new EventBus(),f=jest.fn(); b.on('e',f); b.emit('e',1); expect(f).toHaveBeenCalledWith(1); });\n    test('off removes', () => { const b=new EventBus(),f=jest.fn(); b.on('e',f); b.off('e',f); b.emit('e',1); expect(f).not.toHaveBeenCalled(); });\n    test('off keeps others', () => { const b=new EventBus(),f1=jest.fn(),f2=jest.fn(); b.on('e',f1); b.on('e',f2); b.off('e',f1); b.emit('e','x'); expect(f1).not.toHaveBeenCalled(); expect(f2).toHaveBeenCalledWith('x'); });\n});\n",
            extra_deps: &[],
        },
        SweCase {
            id: "TS-06", lang: "typescript",
            title: "fetchWithRetry لا يرمي بعد exhaustion",
            difficulty: Difficulty::Easy,
            source_file: "src/apiClient.ts",
            source_code: "export async function fetchWithRetry(url:string, retries=3): Promise<any> {\n    for(let i=0;i<retries;i++){\n        const r=await fetch(url);\n        if(r.ok) return r.json();\n    }\n}\n",
            test_file: "src/apiClient.test.ts",
            test_code: "(global as any).fetch=jest.fn();\nimport {fetchWithRetry} from './apiClient';\ndescribe('retry', () => {\n    beforeEach(()=>jest.clearAllMocks());\n    test('success', async () => {\n        (global.fetch as jest.Mock).mockResolvedValueOnce({ok:true,json:async()=>({id:1})});\n        expect(await fetchWithRetry('http://x')).toEqual({id:1});\n    });\n    test('retry then succeed', async () => {\n        (global.fetch as jest.Mock).mockResolvedValueOnce({ok:false}).mockResolvedValueOnce({ok:true,json:async()=>({ok:true})});\n        expect(await fetchWithRetry('http://x',3)).toEqual({ok:true});\n    });\n    test('throws after exhaustion', async () => {\n        (global.fetch as jest.Mock).mockResolvedValue({ok:false,status:503});\n        await expect(fetchWithRetry('http://fail',3)).rejects.toThrow();\n    });\n});\n",
            extra_deps: &[],
        },
    ]
}
