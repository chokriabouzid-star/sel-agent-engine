#![allow(dead_code)]
mod bench_realworld;
mod workspace_oracle;
mod bench_compile;
mod llm_engine;
// src/main.rs — SEL Agent v7.3.0
mod agent;
mod chunker;
mod context;
mod environment;
mod evaluator;
mod executor;
mod goal_parser;

mod memory;
mod protocol;
mod scaffold_engine;
mod scanner;
mod manifest;
mod types;
mod constitution;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "sel-agent", version = "7.3.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        goal: String,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
        #[arg(long, default_value = "false")]
        dry_run: bool,
        #[arg(long)]
        ref_file: Option<PathBuf>,
        #[arg(long, value_delimiter = ',')]
        focus: Vec<String>,
    },
    Health,
    Stress {
        #[arg(long, default_value = "3")]
        max_repairs: u8,
    },
    Bench {
        #[arg(long, default_value = "all")]
        suite: String,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
        #[arg(long, default_value = "1")]
        iterations: u8,
    },
    Scan {
        /// مسار المشروع
        #[arg(long, default_value = ".")]
        workspace: String,
        /// إخراج JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Compare {
        #[arg(long, value_delimiter = ',')]
        models: Vec<String>,
        #[arg(long, default_value = "python")]
        suite: String,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
    },
    Plan {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
    },
    #[command(name = "bench-real-world")]
    BenchRealWorld {
        #[arg(long)]
        tier: Option<u8>,
        #[arg(long, default_value = "6")]
        max_repairs: u8,
    },
}

async fn run_health(api_key: &str) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent v7.3.0 — Health Check                   ║");
    println!("╚══════════════════════════════════════════╝\n");
    // Provider info في الـ bench
    {
        let mdl = std::env::var("SEL_MODEL").unwrap_or_else(|_| "kimi".to_string());
        let (_ep, _key) = if let Ok(base) = std::env::var("SEL_API_BASE") {
            let k = std::env::var("SEL_API_KEY").unwrap_or_default();
            (base, k)
        } else if mdl.contains("gemini") || mdl.starts_with("models/") {
            let k = std::env::var("GEMINI_API_KEY").unwrap_or_default();
            (
                "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
                k,
            )
        } else {
            let k = std::env::var("GROQ_API_KEY").unwrap_or_default();
            ("https://api.groq.com/openai/v1".to_string(), k)
        };
        // print_provider_info removed(&ep, &mdl, &key);
        println!();
    }

    let internet = reqwest::Client::new()
        .get("https://1.1.1.1")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    match internet {
        Ok(_) => println!("🌐 Internet:     {}", "✅ Connected".green()),
        Err(_) => println!("🌐 Internet:     {}", "❌ No connection".red()),
    }

    let groq = reqwest::Client::new()
        .get("https://api.groq.com")
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    match groq {
        Ok(_) => println!("🔌 Groq Server:  {}", "✅ Reachable".green()),
        Err(_) => println!("🔌 Groq Server:  {}", "❌ Unreachable".red()),
    }

    let api_key_str = if api_key.is_empty() {
        std::env::var("OPENROUTER_API_KEY")
            .or_else(|_| std::env::var("GEMINI_API_KEY"))
            .or_else(|_| std::env::var("GROQ_API_KEY"))
            .unwrap_or_default()
    } else { api_key.to_string() };
    let api_key = api_key_str.as_str();
    let key_preview = if api_key.len() > 8 {
        format!("{}...", &api_key[..8])
    } else {
        "???".to_string()
    };
    println!("🔑 API Key:      {} ({})", "✅ Set".green(), key_preview);

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} 🤖 Model:        Testing response...")
            .unwrap(),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    let mut llm = llm_engine::LlmEngine::from_env();
    let test_msg = types::Message::user("Reply with exactly: PONG".to_string());
    match llm.call(&[test_msg]).await {
        Ok(resp) => {
            pb.finish_and_clear();
            if !resp.is_empty() {
                println!("🤖 Model:        {}", "✅ Responding".green());
            } else {
                println!("🤖 Model:        {}", "⚠️  Empty response".yellow());
            }
        }
        Err(e) => {
            pb.finish_and_clear();
            println!("🤖 Model:        {} — {}", "❌ Failed".red(), e);
        }
    }

    let binary = std::env::current_exe().unwrap_or_default();
    println!(
        "⚙️  SEL Binary:   {} ({})",
        "✅ Built".green(),
        binary.display()
    );
    println!();
    Ok(())
}

async fn run_bench(api_key: &str, suite: &str, max_repairs: u8, iterations: u8) -> Result<()> {
    let all_cases: &[(&str, &str, &str)] = &[
        // Python
        ("python", "broken import",    "Create Python file importing from math_utils import add. Create math_utils.py with add(a,b) function. Write pytest test. Run tests."),
        ("python", "wrong assertion",  "Create Python function double(x) returning x*2. Write pytest test asserting double(3)==6. Run tests."),
        ("python", "wrong signature",  "Create Python function greet(name) returning f'Hi {name}'. Write pytest test expecting greet('Alice')=='Hi Alice'. Run tests."),
        ("python", "wrong logic",      "Create Python function is_even(n) returning n%2==0. Write pytest test for is_even(4)==True and is_even(3)==False. Run tests."),
        ("python", "type mismatch",    "Create Python function add(a,b) returning a+b for integers. Write pytest test expecting add(2,3)==5. Run tests."),
        ("python", "missing closing",  "Create Python function factorial(n) with base case n==0 returns 1. Write pytest test for factorial(5)==120. Run tests."),
        ("python", "undefined func",   "Create Python module with helper() returning 42. Write pytest test asserting helper()==42. Run tests."),
        ("python", "wrong logic 2",    "Create Python function max_of_three(a,b,c) returning max(a,b,c). Write pytest test. Run tests."),
        ("python", "syntax error",     "Create Python function add(a,b) returning a+b with correct syntax. Write pytest test. Run tests."),
        ("python", "wrong return",     "Create Python function reverse_string(s) returning s[::-1]. Write pytest test expecting reverse_string('hello')=='olleh'. Run tests."),
        ("python", "missing function", "Create Python class Stack with push(item) and pop() methods. Write pytest test. Run tests."),
        ("python", "runtime error",    "Create Python function divide(a,b) returning None if b==0 else a/b. Write pytest tests: test divide(10,2)==5.0 AND divide(10,0)==None (both branches required). Run tests."),
        // Go
        ("go", "go add",       "Create Go package main with Add(a,b int) int. Create go.mod with module gotest and go 1.21. Write _test.go testing Add(2,3)==5 and Add(-1,1)==0. Run go test."),
        ("go", "go fizzbuzz",  "Create Go package main with FizzBuzz(n int) string returning Fizz/Buzz/FizzBuzz/number. Create go.mod module gotest go 1.21. Write _test.go with 4 test cases using only t.Errorf (no fmt import). Run go test."),
        ("go", "go reverse",   "Create Go package main with Reverse(s string) string. Create go.mod module gotest go 1.21. Write _test.go using only t.Errorf (no fmt): test Reverse(\"hello\")=\"olleh\" and Reverse(\"\")=\"\". Run go test."),
        ("go", "go divide",    "Create Go package main with Divide(a,b float64) (float64,error) returning error if b==0. Create go.mod module gotest go 1.21. Write _test.go testing normal and zero cases. Run go test."),
        // Node
        ("node", "node add",        "Create Node.js CommonJS module math.js exporting add(a,b). Create package.json with jest. Write math.test.js testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
        ("node", "node palindrome", "Create Node.js CommonJS module palindrome.js exporting isPalindrome(s). Create package.json with jest. Write test file testing racecar==true and hello==false. Run npm test."),
        ("node", "node factorial",  "Create Node.js CommonJS module factorial.js exporting factorial(n) with base case 0==1. Create package.json with jest. Write test for factorial(5)==120 and factorial(0)==1. Run npm test."),
        ("node", "node filter",     "Create Node.js CommonJS module filter.js exporting filterEven(arr) returning even numbers. Create package.json with jest. Write test with arrays including empty array case. Run npm test."),
        // TypeScript
        ("typescript", "ts add",        "Create TypeScript file math.ts exporting function add(a:number,b:number):number. Create package.json with jest and ts-jest. Create tsconfig.json. Write math.test.ts testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
        ("typescript", "ts palindrome", "Create TypeScript file palindrome.ts exporting function isPalindrome(s:string):boolean. Create package.json with jest and ts-jest. Create tsconfig.json. Write palindrome.test.ts testing racecar===true and hello===false. Run npm test."),
        ("typescript", "ts factorial",  "Create TypeScript file factorial.ts exporting function factorial(n:number):number with base case 0 returns 1. Create package.json with jest and ts-jest. Create tsconfig.json. Write factorial.test.ts testing factorial(5)===120 and factorial(0)===1. Run npm test."),
        ("typescript", "ts stack",      "Create TypeScript file stack.ts exporting class Stack<T> with push(item:T) pop():T|undefined and isEmpty():boolean. Create package.json with jest and ts-jest. Create tsconfig.json. Write stack.test.ts with push/pop/isEmpty tests. Run npm test."),
        // Rust
        ("rust", "rust add",     "Create Rust library crate. Write Cargo.toml with name=rustadd edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32. Write tests module inside lib.rs testing add(2,3)==5 and add(-1,1)==0. Run cargo test."),
        ("rust", "rust fizzbuzz","Create Rust library crate. Write Cargo.toml name=rustfizz edition=2021. Write src/lib.rs with pub fn fizzbuzz(n:u32)->String returning Fizz Buzz FizzBuzz or number. Write tests module with 4 cases. Run cargo test."),
        ("rust", "rust reverse", "Create Rust library crate. Write Cargo.toml name=rustreverse edition=2021. Write src/lib.rs with pub fn reverse(s:&str)->String. Write tests module testing hello->olleh and empty string. Run cargo test."),
        ("rust", "rust stack",   "Create Rust library crate. Write Cargo.toml name=ruststack edition=2021. Write src/lib.rs with pub struct Stack and impl with push pop is_empty. Write tests module. Run cargo test."),
        // Flask / FastAPI
        ("python", "flask hello",   "Create Python Flask app in app.py with GET /hello route returning JSON {\"message\":\"hello world\"}. Create requirements.txt containing only: flask. Write test_app.py using Flask test client: assert response.status_code==200 and response.get_json()[\"message\"]==\"hello world\". Run pytest."),
        ("python", "fastapi route", "Create Python FastAPI app in main.py with GET /hello route returning {\"message\":\"hello\"}. Create requirements.txt containing: fastapi httpx. Write test_main.py using TestClient from fastapi.testclient: assert response.status_code==200 and response.json()[\"message\"]==\"hello\". Run pytest."),
        // Express multi-file
        ("node", "express api",     "Create Node.js Express app in app.js exporting the express app with GET /ping route returning JSON {ok:true}. Create package.json with jest supertest express. Write app.test.js using supertest: assert status 200 and body.ok===true. Run npm test."),
        // TypeScript Express
        ("typescript", "ts express", "Create TypeScript Express app. Write app.ts exporting express app with GET /health route returning JSON {status:\"ok\"}. Create package.json with ts-jest jest typescript express @types/express supertest @types/supertest. Create tsconfig.json. Write app.test.ts using supertest: assert status 200 and body.status===\"ok\". Run npm test."),
        // v7 Feature Checks
        ("v7", "v7_quickfix",    "Create a Python script using the 'requests' library to fetch 'https://httpbin.org/get'. Write a pytest test asserting status_code is 200. Do NOT use pip_install in your execution commands, let the ModuleNotFoundError happen so we test the agent's QuickFix. Run pytest."),
        ("v7", "v7_go_autofix",  "Create Go package main. Write func PrintMessage() that calls fmt.Println(\"Hello\"). STRICT RULE: You must NOT write `import \"fmt\"` anywhere in the file. Leave it missing! Write a test calling the function. Run go test."),
        ("v7", "v7_rust_quotes", "Create Rust library crate with edition 2021. Write pub fn greet() -> &'static str returning 'Hello' (STRICT RULE: you MUST use single quotes around Hello). Write tests module asserting greet() returns it. Run cargo test."),
        ("v7", "v7_unicode",     "Create a Python function that uses a variable named \u{2018}msg\u{2019} and returns \u{201C}smart quotes\u{201D}. Write a pytest test checking its value. Run tests (the agent's sanitize_code should fix these Unicode bounds)."),
    ];

    let cases: Vec<_> = all_cases
        .iter()
        .filter(|(lang, _, _)| suite == "all" || *lang == suite)
        .collect();

    // v5.7: integration suite له دالة منفصلة
    // v7.1: compile suite
    if suite == "compile" {
        return run_compile_bench(max_repairs).await;
    }
    if suite == "integration" {
        return run_integration_bench("", max_repairs).await;
    }

    if cases.is_empty() {
        println!(
            "❌ Unknown suite '{}'. Use: python, go, node, rust, typescript, integration, all",
            suite
        );
        return Ok(());
    }

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Bench v1.8 — suite: {:<14}║", suite);
    println!("╚══════════════════════════════════════════╝\n");

    let total = cases.len();
    let total_runs = total * iterations as usize;
    let mut passed = 0usize;
    let mut total_repairs = 0usize;
    let mut mutation_killed = 0u32;
    let mut mutation_total = 0u32;
    let tmpdir = std::env::temp_dir();

    for iter in 0..iterations {
        if iterations > 1 {
            println!(
                "\n── Iteration {}/{} ──────────────────────────",
                iter + 1,
                iterations
            );
        }
        for (i, (_lang, name, goal)) in cases.iter().enumerate() {
            let workspace = tmpdir.join(format!("sel-bench-{}-{}", iter, i));
            let _ = std::fs::remove_dir_all(&workspace);
            std::fs::create_dir_all(&workspace).ok();
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template(&format!(
                        "{{spinner:.cyan}} 🔬 [{}/{}] {}...",
                        iter + 1,
                        iterations,
                        name
                    ))
                    .unwrap(),
            );
            pb.enable_steady_tick(Duration::from_millis(80));

            let mut agent = crate::agent::Agent::new(
                api_key.to_string(),
                workspace.clone(),
                goal.to_string(),
                max_repairs,
                types::ContextConfig::default(),
            );
            let ok = agent.run().await.is_ok();
            pb.finish_and_clear();

            let repairs = agent.repair_count();
            total_repairs += repairs;
            let ms = agent.mutation_score();
            if ms >= 0.0 {
                mutation_total += 1;
                if ms >= 1.0 {
                    mutation_killed += 1;
                }
            }

            let status = if ok { "✅" } else { "❌" };
            let ms_str = if ms >= 0.0 {
                format!("{:.0}%", ms * 100.0)
            } else {
                "—".to_string()
            };
            println!(
                "   {} {:20} repairs:{} mutation:{}",
                status, name, repairs, ms_str
            );
            if ok {
                passed += 1;
            }
        }
    }

    let success_rate = passed as f64 / total_runs as f64;
    let avg_repairs = total_repairs as f64 / total_runs as f64;
    let mut_score = if mutation_total > 0 {
        mutation_killed as f64 / mutation_total as f64
    } else {
        -1.0
    };
    let quality = if mut_score >= 0.0 {
        success_rate * mut_score
    } else {
        success_rate
    };

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Bench Results                      ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Suite:          {:<23}║", suite);
    println!("║  Iterations:     {:<23}║", iterations);
    println!(
        "║  Passed:         {:<23}║",
        format!("{}/{}", passed, total_runs)
    );
    println!(
        "║  Success Rate:   {:<23}║",
        format!("{:.1}%", success_rate * 100.0)
    );
    println!("║  Avg Repairs:    {:<23}║", format!("{:.1}", avg_repairs));
    println!(
        "║  Mutation Score: {:<23}║",
        if mut_score >= 0.0 {
            format!("{:.0}%", mut_score * 100.0)
        } else {
            "N/A".to_string()
        }
    );
    println!("║  Quality Index:  {:<23}║", format!("{:.2}", quality));
    println!("╚══════════════════════════════════════════╝\n");

    // POST to Observatory
    let model =
        std::env::var("SEL_MODEL").unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string());
    let version = std::env::var("SEL_VERSION").unwrap_or_else(|_| "v1.9".to_string());
    let body = serde_json::json!({
        "version": version,
        "suite": suite,
        "model": model,
        "passed": passed as i64,
        "total": total as i64,
        "success_rate": success_rate,
        "mutation_score": mut_score,
        "avg_repairs": avg_repairs,
        "quality_index": quality,
        "created_at": ""
    });
    let _ = reqwest::Client::new()
        .post("http://localhost:8777/api/bench")
        .json(&body)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await;

    Ok(())
}

async fn run_stress(api_key: &str, max_repairs: u8) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent v7.3.0 — Stress Test                     ║");
    println!("╚══════════════════════════════════════════╝\n");

    let cases: &[(&str, &str)] = &[
        ("broken import",    "Create Python file importing from math_utils import add. Create math_utils.py with add(a,b) function. Write pytest test. Run tests."),
        ("wrong assertion",  "Create Python function double(x) returning x*2. Write pytest test asserting double(3)==6. Run tests."),
        ("wrong signature",  "Create Python function greet(name) returning f'Hi {name}'. Write pytest test expecting greet('Alice')=='Hi Alice'. Run tests."),
        ("wrong logic",      "Create Python function is_even(n) returning n%2==0. Write pytest test for is_even(4)==True and is_even(3)==False. Run tests."),
        ("type mismatch",    "Create Python function add(a,b) returning a+b for integers. Write pytest test expecting add(2,3)==5. Run tests."),
        ("missing closing",  "Create Python function factorial(n) with base case n==0 returns 1. Write pytest test for factorial(5)==120. Run tests."),
        ("undefined func",   "Create Python module with helper() returning 42. Write pytest test asserting helper()==42. Run tests."),
        ("wrong logic 2",    "Create Python function max_of_three(a,b,c) returning max(a,b,c). Write pytest test. Run tests."),
        ("syntax error",     "Create Python function add(a,b) returning a+b with correct syntax. Write pytest test. Run tests."),
        ("wrong return",     "Create Python function reverse_string(s) returning s[::-1]. Write pytest test expecting reverse_string('hello')=='olleh'. Run tests."),
        ("missing function", "Create Python class Stack with push(item) and pop() methods. Write pytest test. Run tests."),
        ("runtime error",    "Create Python function divide(a,b) returning None if b==0 else a/b. Write pytest tests: test divide(10,2)==5.0 AND divide(10,0)==None (both branches required). Run tests."),
        // Go cases
        ("go add",           "Create Go package main with Add(a,b int) int. Create go.mod with module gotest and go 1.21. Write _test.go testing Add(2,3)==5 and Add(-1,1)==0. Run go test."),
        ("go fizzbuzz",      "Create Go package main with FizzBuzz(n int) string returning Fizz/Buzz/FizzBuzz/number. Create go.mod module gotest go 1.21. Write _test.go with 4 test cases using only t.Errorf (no fmt import). Run go test."),
        ("go reverse",       "Create Go package main with Reverse(s string) string. Create go.mod module gotest go 1.21. Write _test.go using only t.Errorf (no fmt): test Reverse(\"hello\")=\"olleh\" and Reverse(\"\")=\"\". Run go test."),
        ("go divide",        "Create Go package main with Divide(a,b float64) (float64,error) returning error if b==0. Create go.mod module gotest go 1.21. Write _test.go testing normal and zero cases. Run go test."),
        // Node cases
        ("node add",         "Create Node.js CommonJS module math.js exporting add(a,b). Create package.json with jest. Write math.test.js testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
        ("node palindrome",  "Create Node.js CommonJS module palindrome.js exporting isPalindrome(s). Create package.json with jest. Write test file testing racecar==true and hello==false. Run npm test."),
        ("node factorial",   "Create Node.js CommonJS module factorial.js exporting factorial(n) with base case 0==1. Create package.json with jest. Write test for factorial(5)==120 and factorial(0)==1. Run npm test."),
        ("node filter",      "Create Node.js CommonJS module filter.js exporting filterEven(arr) returning even numbers. Create package.json with jest. Write test with arrays including empty array case. Run npm test."),
        // Rust cases
        ("rust add",         "Create Rust library crate. Write Cargo.toml with name=rustadd edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32. Write tests module inside lib.rs testing add(2,3)==5 and add(-1,1)==0. Run cargo test."),
        ("rust fizzbuzz",    "Create Rust library crate. Write Cargo.toml name=rustfizz edition=2021. Write src/lib.rs with pub fn fizzbuzz(n:u32)->String returning Fizz Buzz FizzBuzz or number. Write tests module with 4 cases. Run cargo test."),
        ("rust reverse",     "Create Rust library crate. Write Cargo.toml name=rustreverse edition=2021. Write src/lib.rs with pub fn reverse(s:&str)->String. Write tests module testing hello->olleh and empty string. Run cargo test."),
        ("rust stack",       "Create Rust library crate. Write Cargo.toml name=ruststack edition=2021. Write src/lib.rs with pub struct Stack and impl with push pop is_empty. Write tests module. Run cargo test."),
    ];

    let total = cases.len();
    let mut passed = 0usize;
    let mut total_repairs = 0usize;
    let tmpdir = std::env::temp_dir();

    for (i, (name, goal)) in cases.iter().enumerate() {
        let workspace = tmpdir.join(format!("sel-stress-{}", i));
        let _ = std::fs::remove_dir_all(&workspace);

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!("{{spinner:.yellow}} ⏳ Running: {}...", name))
                .unwrap(),
        );
        pb.enable_steady_tick(Duration::from_millis(80));

        let mut ag = agent::Agent::new(
            api_key.to_string(),
            workspace.clone(),
            goal.to_string(),
            max_repairs,
            types::ContextConfig::default(),
        );
        let result = ag.run().await;
        pb.finish_and_clear();

        match result {
            Ok(_) => {
                let repairs = ag.repair_count();
                total_repairs += repairs;
                println!("   ✅ {} (repairs: {})", name.green(), repairs);
                passed += 1;
            }
            Err(_) => println!("   ❌ {}", name.red()),
        }
        let _ = std::fs::remove_dir_all(&workspace);
        if i < total - 1 {
            tokio::time::sleep(Duration::from_secs(12)).await;
        }
    }

    let avg = if passed > 0 {
        total_repairs as f64 / passed as f64
    } else {
        0.0
    };
    println!(
        "\n=== Stress Results: {}/{} passed | avg repairs: {:.1} ===\n",
        passed, total, avg
    );
    Ok(())
}

async fn run_integration_bench(api_key: &str, max_repairs: u8) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Integration Bench v5.7             ║");
    println!("║   Phase1: Build → Phase2: Patch          ║");
    println!("╚══════════════════════════════════════════╝\n");

    let cases: &[(&str, &str, &str, &str)] = &[
        (
            "patch + ref_file",
            "Create Rust library crate. Write Cargo.toml with name=rustcalc edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32 returning a+b. Write tests module inside lib.rs testing add(2,3)==5. Run cargo test.",
            "The crate rustcalc already exists in src/lib.rs. Use patch_file to add pub fn multiply(a:i32,b:i32)->i32 returning a*b to src/lib.rs. Do NOT use write_file. Add 2 tests for multiply inside the tests module. Run cargo test.",
            "src/lib.rs"
        ),
        (
            "fix_rust_string_literals",
            "Create Rust library crate. Write Cargo.toml with name=rustgreet edition=2021. Write src/lib.rs with pub fn greet(name:&str)->String returning format!(\"Hello {}\", name). Write tests module testing greet(\"World\")==\"Hello World\". Run cargo test.",
            "The crate rustgreet already exists. Use patch_file to add pub fn farewell()->&'static str to src/lib.rs. The function must return \"BYE\". Add 1 test asserting farewell()==\"BYE\". Run cargo test.",
            "src/lib.rs"
        ),
        (
            "duplicate detection",
            "Create Rust library crate. Write Cargo.toml with name=rustdup edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32 returning a+b. Write tests module testing add(2,3)==5. Run cargo test.",
            "The crate rustdup already exists with add() already defined. Use patch_file to add pub fn subtract(a:i32,b:i32)->i32 returning a-b to src/lib.rs. Do NOT redefine add(). Add 2 tests for subtract only. Run cargo test.",
            "src/lib.rs"
        ),
        (
            "skeleton multi-file",
            "Create Rust library crate with 2 source files. Write Cargo.toml name=rustmulti edition=2021. Write src/lib.rs with: pub mod math; pub use math::add;. Write src/math.rs with pub fn add(a:i32,b:i32)->i32 returning a+b. Write tests/math_test.rs testing add(2,3)==5. Run cargo test.",
            "The crate rustmulti already exists with src/lib.rs and src/math.rs. Use patch_file to add pub fn multiply(a:i32,b:i32)->i32 to src/math.rs ONLY. Do NOT touch src/lib.rs. Add 2 tests in tests/math_test.rs. Run cargo test.",
            "src/math.rs"
        ),
        (
            "flask add route",
            "Create Python Flask app in app.py with GET /hello route returning JSON {\"message\":\"hello\"}. Create requirements.txt with only: flask. Write test_app.py using Flask test client testing GET /hello returns 200 and message==\"hello\". Run pytest.",
            "The Flask app already exists in app.py. Use patch_file to add GET /goodbye route returning JSON {\"message\":\"goodbye\"} to app.py. Add 1 new test in test_app.py for GET /goodbye returns 200. Do NOT modify existing tests. Run pytest.",
            "app.py"
        ),
        (
            "fastapi add endpoint",
            "Create Python FastAPI app in main.py with GET /hello route returning {\"message\":\"hello\"}. Create requirements.txt with: fastapi httpx. Write test_main.py using TestClient from fastapi.testclient testing GET /hello returns 200. Run pytest.",
            "The FastAPI app already exists in main.py. Use patch_file to add GET /bye route returning {\"message\":\"bye\"} to main.py. Add 1 new test in test_main.py for GET /bye. Do NOT modify existing tests. Run pytest.",
            "main.py"
        ),
        (
            "marketing bot patch reddit",
            "Create a Node.js TypeScript Marketing Bot. Architecture: 1. src/database.ts with in-memory CampaignStore class storing {id,platform,url,date}. 2. src/platforms/devto.ts with DevToClient class taking apiKey, having postArticle(title:string,url:string):Promise<string> method using axios (mock-friendly). 3. src/scheduler.ts with Scheduler class that takes a platform client and has schedule(campaign) method. 4. src/index.ts exporting all. Write src/scheduler.test.ts using jest.mock for axios testing schedule() works. Extra deps: axios",
            "The Marketing Bot already exists with src/database.ts src/platforms/devto.ts src/scheduler.ts src/index.ts. Use patch_file or write_file to ADD src/platforms/reddit.ts with RedditClient class taking apiKey, having postLink(title:string,url:string,subreddit:string):Promise<string> method (axios-based). Add src/platforms/reddit.test.ts using jest.mock for axios testing postLink returns a string id. Do NOT modify existing files except src/index.ts to export RedditClient. Run npm test.",
            "src/scheduler.ts"
        ),
    ];

    let total = cases.len();
    let mut passed = 0usize;
    let mut total_repairs = 0usize;
    let mut phase1_passed = 0usize;
    let mut phase2_passed = 0usize;
    let tmpdir = std::env::temp_dir();

    for (i, (name, goal1, goal2, ref_hint)) in cases.iter().enumerate() {
        println!(
            "\n── Test {}/{}: {} ──────────────────────",
            i + 1,
            total,
            name
        );

        // Phase 1: Build
        let workspace = tmpdir.join(format!("sel-integration-{}", i));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).ok();

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!("{{spinner:.cyan}} Phase1 [{}]...", name))
                .unwrap(),
        );
        pb.enable_steady_tick(Duration::from_millis(80));

        let mut agent1 = crate::agent::Agent::new(
            api_key.to_string(),
            workspace.clone(),
            goal1.to_string(),
            max_repairs,
            types::ContextConfig::default(),
        );
        let ok1 = agent1.run().await.is_ok();
        pb.finish_and_clear();

        let repairs1 = agent1.repair_count();
        if ok1 {
            phase1_passed += 1;
            println!("   ✅ Phase1 passed (repairs: {})", repairs1);
        } else {
            println!(
                "   ❌ Phase1 FAILED (repairs: {}) — skipping Phase2",
                repairs1
            );
            total_repairs += repairs1;
            continue;
        }

        // Phase 2: Patch
        let ref_file_path = workspace.join(ref_hint);
        let ctx_config = types::ContextConfig {
            ref_file: if ref_file_path.exists() {
                Some(ref_file_path)
            } else {
                None
            },
            focus_paths: vec!["src/".to_string()],
            ..Default::default()
        };

        let pb2 = ProgressBar::new_spinner();
        pb2.set_style(
            ProgressStyle::default_spinner()
                .template(&format!("{{spinner:.green}} Phase2 [{}]...", name))
                .unwrap(),
        );
        pb2.enable_steady_tick(Duration::from_millis(80));

        let mut agent2 = crate::agent::Agent::new(
            api_key.to_string(),
            workspace.clone(),
            goal2.to_string(),
            max_repairs,
            ctx_config,
        );
        let ok2 = agent2.run().await.is_ok();
        pb2.finish_and_clear();

        let repairs2 = agent2.repair_count();
        total_repairs += repairs1 + repairs2;

        if ok2 {
            phase2_passed += 1;
            passed += 1;
            println!("   ✅ Phase2 passed (repairs: {})", repairs2);
        } else {
            println!("   ❌ Phase2 FAILED (repairs: {})", repairs2);
        }

        let _ = std::fs::remove_dir_all(&workspace);
    }

    let avg_repairs = if total > 0 {
        total_repairs as f64 / total as f64
    } else {
        0.0
    };
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   Integration Bench Results               ║");
    println!("╠══════════════════════════════════════════╣");
    println!(
        "║  Tests:          {:<23}║",
        format!("{} cases x 2 phases", total)
    );
    println!(
        "║  Phase1 passed:  {:<23}║",
        format!("{}/{}", phase1_passed, total)
    );
    println!(
        "║  Phase2 passed:  {:<23}║",
        format!("{}/{}", phase2_passed, total)
    );
    println!(
        "║  Full passed:    {:<23}║",
        format!("{}/{}", passed, total)
    );
    println!("║  Avg Repairs:    {:<23}║", format!("{:.1}", avg_repairs));
    println!("╚══════════════════════════════════════════╝\n");

    Ok(())
}

async fn run_compare(models: &[String], suite: &str, max_repairs: u8) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent v7.3.0 — Model Comparison       ║");
    println!("╚══════════════════════════════════════════╝\n");
    println!("   Models:  {:?}", models);
    println!("   Suite:   {}", suite);
    println!();

    #[derive(Debug)]
    struct ModelResult {
        model: String,
        passed: usize,
        total: usize,
        avg_repairs: f64,
        mut_score: f64,
        quality: f64,
        elapsed_secs: u64,
        retries: u32,
        connection_errors: u32,
        rate_limits: u32,
        timeouts: u32,
    }

    let all_cases: &[(&str, &str, &str)] = &[
        ("python", "broken import",   "Create Python file importing from math_utils import add. Create math_utils.py with add(a,b) function. Write pytest test. Run tests."),
        ("python", "wrong logic",     "Create Python function is_even(n) returning n%2==0. Write pytest test for is_even(4)==True and is_even(3)==False. Run tests."),
        ("python", "wrong return",    "Create Python function reverse_string(s) returning s[::-1]. Write pytest test expecting reverse_string('hello')=='olleh'. Run tests."),
        ("rust",   "rust add",        "Create Rust library crate. Write Cargo.toml with name=rustadd edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32. Write tests module inside lib.rs testing add(2,3)==5 and add(-1,1)==0. Run cargo test."),
        ("go",     "go add",          "Create Go package main with Add(a,b int) int. Create go.mod with module gotest and go 1.21. Write _test.go testing Add(2,3)==5 and Add(-1,1)==0. Run go test."),
        ("node",   "node add",        "Create Node.js CommonJS module math.js exporting add(a,b). Create package.json with jest. Write math.test.js testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
    ];

    let cases: Vec<_> = all_cases
        .iter()
        .filter(|(lang, _, _)| suite == "all" || *lang == suite)
        .collect();

    if cases.is_empty() {
        println!(
            "❌ Unknown suite '{}'. Use: python, go, node, rust, typescript, all",
            suite
        );
        return Ok(());
    }

    let tmpdir = std::env::temp_dir();
    let mut results: Vec<ModelResult> = Vec::new();

    for model_alias in models {
        println!(
            "\n🤖 Testing model: {} ──────────────────────────",
            model_alias
        );

        let model_cfg = crate::llm_engine::ModelConfig::from_alias(model_alias);
        let api_key = std::env::var(&model_cfg.env_key).unwrap_or_else(|_| {
            println!(
                "   ⚠ {} غير موجود — تخطي النموذج {}",
                model_cfg.env_key, model_alias
            );
            String::new()
        });

        if api_key.is_empty() {
            continue;
        }

        let mut passed = 0usize;
        let mut total_repairs = 0usize;
        let mut mutation_killed = 0u32;
        let mut mutation_total = 0u32;
        let mut total_retries = 0usize;
        let mut total_connection_errors = 0usize;
        let mut total_rate_limits = 0usize;
        let mut total_timeouts = 0usize;
        let total = cases.len();
        let start = std::time::Instant::now();

        for (i, (_lang, name, goal)) in cases.iter().enumerate() {
            let workspace = tmpdir.join(format!("sel-cmp-{}-{}", model_alias, i));
            let _ = std::fs::remove_dir_all(&workspace);
            std::fs::create_dir_all(&workspace).expect("failed to create workspace");

            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template(&format!(
                        "{{spinner:.cyan}} [{}/{}] {}...",
                        i + 1,
                        total,
                        name
                    ))
                    .unwrap(),
            );
            pb.enable_steady_tick(Duration::from_millis(80));

            let mut agent = crate::agent::Agent::new_with_model(
                api_key.clone(),
                model_alias.clone(),
                workspace.clone(),
                goal.to_string(),
                max_repairs,
                types::ContextConfig::default(),
            );
            let run_result = agent.run().await;
            let ok = run_result.is_ok();
            pb.finish_and_clear();

            if let Err(ref e) = run_result {
                println!(
                    "   ❌ {:20} FAILED: {}",
                    name,
                    &e.to_string()[..e.to_string().len().min(80)]
                );
                let _ = std::fs::remove_dir_all(&workspace);
                continue;
            }

            let repairs = agent.repair_count();
            total_repairs += repairs;
            // v6.1: تراكم إحصائيات الاتصال
            let cstats = agent.call_stats();
            total_retries += cstats.retries as usize;
            total_connection_errors += cstats.connection_errors as usize;
            total_rate_limits += cstats.rate_limits as usize;
            total_timeouts += cstats.timeouts as usize;
            let ms = agent.mutation_score();
            if ms >= 0.0 {
                mutation_total += 1;
                if ms >= 1.0 {
                    mutation_killed += 1;
                }
            }

            let status = if ok { "✅" } else { "❌" };
            let ms_str = if ms >= 0.0 {
                format!("{:.0}%", ms * 100.0)
            } else {
                "—".to_string()
            };
            println!(
                "   {} {:20} repairs:{} mutation:{}",
                status, name, repairs, ms_str
            );
            if ok {
                passed += 1;
            }
            let _ = std::fs::remove_dir_all(&workspace);
        }

        let elapsed = start.elapsed().as_secs();
        let success_rate = passed as f64 / total as f64;
        let avg_repairs = total_repairs as f64 / total as f64;
        let mut_score = if mutation_total > 0 {
            mutation_killed as f64 / mutation_total as f64
        } else {
            -1.0
        };
        let quality = if mut_score >= 0.0 {
            success_rate * mut_score
        } else {
            success_rate
        };

        results.push(ModelResult {
            model: model_cfg.model_id.clone(),
            passed,
            total,
            avg_repairs,
            mut_score,
            quality,
            elapsed_secs: elapsed,
            retries: total_retries as u32,
            connection_errors: total_connection_errors as u32,
            rate_limits: total_rate_limits as u32,
            timeouts: total_timeouts as u32,
        });
    }

    // ── v6.1: RAS + DTO ──
    let max_time = results.iter().map(|r| r.elapsed_secs).max().unwrap_or(1);
    let mut scores: Vec<evaluator::ModelScore> = results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let raw = evaluator::RawMetrics {
                tests_passed: r.passed as u32,
                tests_total: r.total as u32,
                mutation_score: if r.mut_score >= 0.0 { r.mut_score } else { 0.0 },
                repairs: (r.avg_repairs * r.total as f64).round() as u32,
                retries: r.retries,
                connection_errors: r.connection_errors,
                rate_limits: r.rate_limits,
                timeouts: r.timeouts,
                elapsed_secs: r.elapsed_secs,
            };
            evaluator::ModelScore::from_metrics(&r.model, &format!("run-{}", i), &raw, max_time)
        })
        .collect();

    scores = evaluator::rank_models(scores);
    evaluator::print_comparison_table(&scores);

    if let Some(best) = scores.iter().find(|s| !s.unstable) {
        println!(
            "🏆 أفضل نموذج: {} (Composite: {:.3} | Correct: {:.2} | Reliable: {:.2})\n",
            best.model, best.composite, best.correctness, best.reliability
        );
    } else {
        println!("⚠ جميع النماذج غير مستقرة — لا يوجد فائز\n");
    }

    Ok(())
}

async fn run_plan(api_key: &str,
    workspace: &std::path::Path,
    plan_file: &std::path::Path,
    max_repairs: u8,
) -> Result<()> {
    let content = std::fs::read_to_string(plan_file)
        .map_err(|e| anyhow::anyhow!("Cannot read plan file: {}", e))?;

    // parse lines: "- [ ] goal text" or "- [x] done"
    let tasks: Vec<String> = content
        .lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.starts_with("- [ ]") {
                Some(t[5..].trim().to_string())
            } else if t.starts_with("* [ ]") {
                Some(t[5..].trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())
        .collect();

    if tasks.is_empty() {
        println!("\n❌ No pending tasks found in plan file.");
        println!("   Use format: - [ ] your goal here");
        return Ok(());
    }

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent — Markdown Plan Runner        ║");
    println!("╠══════════════════════════════════════════╣");
    println!(
        "║  Plan:       {:<27}║",
        plan_file.file_name().unwrap_or_default().to_string_lossy()
    );
    println!("║  Tasks:      {:<27}║", tasks.len());
    println!(
        "║  Workspace:  {:<27}║",
        workspace
            .display()
            .to_string()
            .chars()
            .take(27)
            .collect::<String>()
    );
    println!("╚══════════════════════════════════════════╝\n");

    std::fs::create_dir_all(workspace).ok();

    let mut passed = 0usize;
    let mut total_repairs = 0usize;

    for (i, task) in tasks.iter().enumerate() {
        println!(
            "\n── Task {}/{} ─────────────────────────────────",
            i + 1,
            tasks.len()
        );
        println!("   📋 {}", &task.chars().take(80).collect::<String>());

        let pb = indicatif::ProgressBar::new_spinner();
        pb.set_style(
            indicatif::ProgressStyle::default_spinner()
                .template(&format!(
                    "{{spinner:.cyan}} ⚙️  Task [{}/{}]...",
                    i + 1,
                    tasks.len()
                ))
                .unwrap(),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(80));

        let mut ag = agent::Agent::new(
            api_key.to_string(),
            workspace.to_path_buf(),
            task.clone(),
            max_repairs,
            types::ContextConfig::default(),
        );
        let ok = ag.run().await.is_ok();
        pb.finish_and_clear();

        let repairs = ag.repair_count();
        total_repairs += repairs;

        if ok {
            passed += 1;
            println!("   ✅ Passed (repairs: {})", repairs);
        } else {
            println!("   ❌ Failed (repairs: {})", repairs);
        }
    }

    let avg_repairs = if tasks.len() > 0 {
        total_repairs as f64 / tasks.len() as f64
    } else {
        0.0
    };

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   Plan Results                            ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Tasks:      {:<27}║", tasks.len());
    println!(
        "║  Passed:     {:<27}║",
        format!("{}/{}", passed, tasks.len())
    );
    println!("║  Avg Repairs:{:<27}║", format!("{:.1}", avg_repairs));
    println!("╚══════════════════════════════════════════╝\n");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Health => {
            run_health(&crate::llm_engine::LlmEngine::from_env().primary_name()).await?;
        }
        Commands::Bench {
            suite,
            max_repairs,
            iterations,
        } => {
            run_bench("", &suite, max_repairs, iterations).await?;
        }
        Commands::Stress { max_repairs } => {
            run_stress("", max_repairs).await?;
        }
        Commands::Scan { workspace, json } => {
            cmd_scan(&workspace, json);
        }
        Commands::Compare {
            models,
            suite,
            max_repairs,
        } => {
            run_compare(&models, &suite, max_repairs).await?;
        }
        Commands::Plan {
            workspace,
            plan,
            max_repairs,
        } => {
            run_plan("", &workspace, &plan, max_repairs).await?;
        }
        Commands::BenchRealWorld { tier, max_repairs } => {
            crate::bench_realworld::run_bench_realworld("", tier, max_repairs).await?;
        }
        Commands::Run {
            workspace,
            goal,
            max_repairs,
            dry_run,
            ref_file,
            focus,
        } => {
            println!("\n╔══════════════════════════════════════════╗");
            println!("║   SEL Agent v7.3.0 — State Machine Engine   ║");
            println!("╚══════════════════════════════════════════╝");
            println!("\n📋 Goal: \"{}\"", goal);
            // Provider info
            {
                let engine = crate::llm_engine::LlmEngine::from_env();
                engine.print_info();
            }
            println!("   Workspace:   {}", workspace.display());
            println!("   Max repairs: {}", max_repairs);
            if let Some(ref rf) = ref_file {
                println!("   Ref file:    {}", rf.display());
            }
            if !focus.is_empty() {
                println!("   Focus:       {:?}", focus);
            }

            if dry_run {
                println!("   Mode:         🔍 DRY RUN\n");
                let mut llm = llm_engine::LlmEngine::from_env();
                let prompt = format!("Goal: {}\n\nProvide the complete execution plan.", goal);
                match llm.call(&[types::Message::user(prompt)]).await {
                    Ok(response) => match protocol::parse(&response) {
                        Ok(plan) => {
                            println!("📋 Plan preview ({} commands):\n", plan.commands.len());
                            for (i, cmd) in plan.commands.iter().enumerate() {
                                println!("  [{}/{}] {}", i + 1, plan.commands.len(), cmd.label());
                            }
                            println!("\n✅ DRY RUN complete.");
                        }
                        Err(e) => println!("❌ Plan parse error: {}", e),
                    },
                    Err(e) => println!("❌ LLM error: {}", e),
                }
                return Ok(());
            }

            std::fs::create_dir_all(&workspace)?;
            // v5.4: توسيع ~ في مسار ref-file
            let ref_file_expanded = ref_file.as_ref().map(|p| {
                let s = p.to_string_lossy();
                if s.starts_with("~/") {
                    if let Ok(home) = std::env::var("HOME") {
                        return std::path::PathBuf::from(format!("{}/{}", home, &s[2..]));
                    }
                }
                p.clone()
            });
            let ctx_config = types::ContextConfig {
                ref_file: ref_file_expanded,
                focus_paths: focus.clone(),
                ..Default::default()
            };
            let mut ag = agent::Agent::new(String::new(), workspace, goal, max_repairs, ctx_config);
            ag.run().await?;
        }
    }
    Ok(())
}

// ─── scan command (v6.2) ───────────────────────────────────────────────────

fn cmd_scan(workspace: &str, json: bool) {
    use crate::scanner::scan_project;
    use std::path::Path;

    let path = Path::new(workspace);
    if !path.exists() {
        eprintln!("❌ Workspace not found: {}", workspace);
        std::process::exit(1);
    }

    let profile = scan_project(path);

    if json {
        println!("{}", serde_json::to_string_pretty(&profile).unwrap());
        return;
    }

    // human-readable output
    let conf_bar = confidence_bar(profile.confidence);
    println!();
    println!("📁 Project  : {}", profile.project_name);
    println!("🔤 Language : {}", profile.language);
    println!(
        "📦 Manifest : {}",
        profile
            .dependency_file
            .as_ref()
            .map(|p: &std::path::PathBuf| p.display().to_string())
            .unwrap_or_else(|| "—".to_string())
    );
    println!(
        "📍 Entry    : {}",
        if profile.entry_points.is_empty() {
            "—".to_string()
        } else {
            profile
                .entry_points
                .iter()
                .map(|p: &std::path::PathBuf| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!(
        "🧪 Tests    : {} {}",
        if profile.has_tests { "✅" } else { "❌" },
        profile.test_framework.as_deref().unwrap_or("")
    );
    println!(
        "🏗  Build    : {}",
        profile.build_cmd.as_deref().unwrap_or("—")
    );
    println!(
        "✅ Test cmd : {}",
        profile.test_cmd.as_deref().unwrap_or("—")
    );
    println!(
        "🎯 Confid.  : {:.0}%  {}",
        profile.confidence * 100.0,
        conf_bar
    );
    println!();
}

fn confidence_bar(c: f32) -> String {
    let filled = (c * 10.0).round() as usize;
    let empty = 10 - filled.min(10);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

// ══════════════════════════════════════════════════════
// Compile Bench — v7.1
// ══════════════════════════════════════════════════════

async fn run_compile_bench(max_repairs: u8) -> Result<()> {
    use crate::bench_compile::{all_cases, setup_case, check_result};

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Bench — suite: compile (v7.1)      ║");
    println!("╚══════════════════════════════════════════╝\n");

    let cases = all_cases();
    let total = cases.len();
    let mut passed = 0usize;
    let mut results: Vec<(String, String, usize, bool, String)> = Vec::new();
    let tmpdir = std::env::temp_dir();

    for (i, case) in cases.iter().enumerate() {
        let workspace = tmpdir.join(format!("sel-compile-{}", i));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).ok();

        // Setup: كتابة الملفات المكسورة
        setup_case(case.name, &workspace);

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!(
                    "{{spinner:.cyan}} 🔬 [{}/{}] {}...",
                    i + 1, total, case.name
                ))
                .unwrap(),
        );
        pb.enable_steady_tick(Duration::from_millis(80));

        // تشغيل Agent مع goal محدد
        let mut agent = crate::agent::Agent::new(
            String::new(),
            workspace.clone(),
            case.goal.to_string(),
            case.max_repairs.min(max_repairs),
            crate::types::ContextConfig::default(),
        );
        let ok = agent.run().await.is_ok();
        pb.finish_and_clear();

        let repairs = agent.repair_count();
        let mutation = agent.mutation_score();

        // فحص النتيجة
        let check = check_result(case.name, &workspace, ok, repairs, mutation);

        let status = if check.passed { "✅" } else { "❌" };
        let mut notes = Vec::new();
        if check.created_wrong_files { notes.push("wrong_files".to_string()); }
        if !check.mutation_ok { notes.push("mutation_fail".to_string()); }
        if repairs > 2 { notes.push(format!("repairs:{}", repairs)); }
        let note_str = if notes.is_empty() { "ok".to_string() } else { notes.join(", ") };

        println!(
            "   {} {:<28} repairs:{}  {}",
            status, case.name, repairs, note_str
        );

        if check.passed { passed += 1; }
        results.push((
            case.name.to_string(),
            status.to_string(),
            repairs,
            check.passed,
            note_str,
        ));

        // تنظيف
        let _ = std::fs::remove_dir_all(&workspace);
    }

    // النتائج النهائية
    let rate = passed as f64 / total as f64 * 100.0;
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   Compile Bench Results                   ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Passed:  {}/{}  ({:.0}%)                   ║", passed, total, rate);
    println!("╠══════════════════════════════════════════╣");

    for (name, status, repairs, _, note) in &results {
        println!("║  {} {:<22} r:{} {}",
            status, name, repairs,
            if note.len() > 15 { &note[..15] } else { note }
        );
    }

    println!("╚══════════════════════════════════════════╝");

    if rate >= 100.0 {
        println!("\n   🏆 v7.1 مستقر تماماً");
    } else if rate >= 75.0 {
        println!("\n   ⚠️  بعض القدرات تحتاج تحسين");
    } else {
        println!("\n   ❌ v7.1 يحتاج مراجعة جدية");
    }

    Ok(())
}
