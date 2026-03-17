// src/main.rs — SEL Agent v1.5
mod context;
mod types;
mod protocol;
mod executor;
mod llm;
mod agent;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "sel-agent", version = "1.5.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(long)] workspace:   PathBuf,
        #[arg(long)] goal:        String,
        #[arg(long, default_value = "3")] max_repairs: u8,
        #[arg(long, default_value = "false")] dry_run: bool,
    },
    Health,
    Stress {
        #[arg(long, default_value = "3")] max_repairs: u8,
    },
    Bench {
        #[arg(long, default_value = "all")] suite: String,
        #[arg(long, default_value = "3")]   max_repairs: u8,
        #[arg(long, default_value = "1")]   iterations: u8,
    },
}

async fn run_health(api_key: &str) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Agent v1.5 — Health Check          ║");
    println!("╚══════════════════════════════════════════╝\n");

    let internet = reqwest::Client::new()
        .get("https://1.1.1.1")
        .timeout(Duration::from_secs(5))
        .send().await;
    match internet {
        Ok(_)  => println!("🌐 Internet:     {}", "✅ Connected".green()),
        Err(_) => println!("🌐 Internet:     {}", "❌ No connection".red()),
    }

    let groq = reqwest::Client::new()
        .get("https://api.groq.com")
        .timeout(Duration::from_secs(5))
        .send().await;
    match groq {
        Ok(_)  => println!("🔌 Groq Server:  {}", "✅ Reachable".green()),
        Err(_) => println!("🔌 Groq Server:  {}", "❌ Unreachable".red()),
    }

    let key_preview = if api_key.len() > 8 {
        format!("{}...", &api_key[..8])
    } else { "???".to_string() };
    println!("🔑 API Key:      {} ({})", "✅ Set".green(), key_preview);

    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner()
        .template("{spinner:.cyan} 🤖 Model:        Testing response...").unwrap());
    pb.enable_steady_tick(Duration::from_millis(100));

    let llm = llm::LlmClient::new(api_key.to_string());
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
    println!("⚙️  SEL Binary:   {} ({})", "✅ Built".green(), binary.display());
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
        ("go", "go fizzbuzz",  "Create Go package main with FizzBuzz(n int) string returning Fizz/Buzz/FizzBuzz/number. Create go.mod module gotest go 1.21. Write _test.go with 4 test cases. Run go test."),
        ("go", "go reverse",   "Create Go package main with Reverse(s string) string. Create go.mod module gotest go 1.21. Write _test.go testing Reverse(hello)==olleh and Reverse()==empty. Run go test."),
        ("go", "go divide",    "Create Go package main with Divide(a,b float64) (float64,error) returning error if b==0. Create go.mod module gotest go 1.21. Write _test.go testing normal and zero cases. Run go test."),
        // Node
        ("node", "node add",        "Create Node.js CommonJS module math.js exporting add(a,b). Create package.json with jest. Write math.test.js testing add(2,3)===5 and add(-1,1)===0. Run npm test."),
        ("node", "node palindrome", "Create Node.js CommonJS module palindrome.js exporting isPalindrome(s). Create package.json with jest. Write test file testing racecar==true and hello==false. Run npm test."),
        ("node", "node factorial",  "Create Node.js CommonJS module factorial.js exporting factorial(n) with base case 0==1. Create package.json with jest. Write test for factorial(5)==120 and factorial(0)==1. Run npm test."),
        ("node", "node filter",     "Create Node.js CommonJS module filter.js exporting filterEven(arr) returning even numbers. Create package.json with jest. Write test with arrays including empty array case. Run npm test."),
        // Rust
        ("rust", "rust add",     "Create Rust library crate. Write Cargo.toml with name=rustadd edition=2021. Write src/lib.rs with pub fn add(a:i32,b:i32)->i32. Write tests module inside lib.rs testing add(2,3)==5 and add(-1,1)==0. Run cargo test."),
        ("rust", "rust fizzbuzz","Create Rust library crate. Write Cargo.toml name=rustfizz edition=2021. Write src/lib.rs with pub fn fizzbuzz(n:u32)->String returning Fizz Buzz FizzBuzz or number. Write tests module with 4 cases. Run cargo test."),
        ("rust", "rust reverse", "Create Rust library crate. Write Cargo.toml name=rustreverse edition=2021. Write src/lib.rs with pub fn reverse(s:&str)->String. Write tests module testing hello->olleh and empty string. Run cargo test."),
        ("rust", "rust stack",   "Create Rust library crate. Write Cargo.toml name=ruststack edition=2021. Write src/lib.rs with pub struct Stack and impl with push pop is_empty. Write tests module. Run cargo test."),
    ];

    let cases: Vec<_> = all_cases.iter().filter(|(lang, _, _)| {
        suite == "all" || *lang == suite
    }).collect();

    if cases.is_empty() {
        println!("❌ Unknown suite '{}'. Use: python, go, node, rust, all", suite);
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
    let mut mutation_total  = 0u32;
    let tmpdir = std::env::temp_dir();

    for iter in 0..iterations {
        if iterations > 1 {
            println!("\n── Iteration {}/{} ──────────────────────────", iter+1, iterations);
        }
        for (i, (_lang, name, goal)) in cases.iter().enumerate() {
            let workspace = tmpdir.join(format!("sel-bench-{}-{}", iter, i));
            let _ = std::fs::remove_dir_all(&workspace);
            let pb = ProgressBar::new_spinner();
            pb.set_style(ProgressStyle::default_spinner()
                .template(&format!("{{spinner:.cyan}} 🔬 [{}/{}] {}...", iter+1, iterations, name)).unwrap());
            pb.enable_steady_tick(Duration::from_millis(80));

            let mut agent = crate::agent::Agent::new(
                api_key.to_string(), workspace.clone(),
                goal.to_string(), max_repairs,
            );
            let ok = agent.run().await.is_ok();
            pb.finish_and_clear();

            let repairs = agent.repair_count();
            total_repairs += repairs;
            let ms = agent.mutation_score();
            if ms >= 0.0 { mutation_total += 1; if ms >= 1.0 { mutation_killed += 1; } }

            let status = if ok { "✅" } else { "❌" };
            let ms_str = if ms >= 0.0 { format!("{:.0}%", ms * 100.0) } else { "—".to_string() };
            println!("   {} {:20} repairs:{} mutation:{}", status, name, repairs, ms_str);
            if ok { passed += 1; }
        }
    }

    let success_rate = passed as f64 / total_runs as f64;
    let avg_repairs  = total_repairs as f64 / total_runs as f64;
    let mut_score    = if mutation_total > 0 { mutation_killed as f64 / mutation_total as f64 } else { -1.0 };
    let quality      = if mut_score >= 0.0 { success_rate * mut_score } else { success_rate };

    println!("\n╔══════════════════════════════════════════╗");
    println!("║   SEL Bench Results                      ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Suite:          {:<23}║", suite);
    println!("║  Iterations:     {:<23}║", iterations);
    println!("║  Passed:         {:<23}║", format!("{}/{}", passed, total_runs));
    println!("║  Success Rate:   {:<23}║", format!("{:.1}%", success_rate * 100.0));
    println!("║  Avg Repairs:    {:<23}║", format!("{:.1}", avg_repairs));
    println!("║  Mutation Score: {:<23}║", if mut_score >= 0.0 { format!("{:.0}%", mut_score * 100.0) } else { "N/A".to_string() });
    println!("║  Quality Index:  {:<23}║", format!("{:.2}", quality));
    println!("╚══════════════════════════════════════════╝\n");

    // POST to Observatory
    let model = std::env::var("SEL_MODEL")
        .unwrap_or_else(|_| "moonshotai/kimi-k2-instruct".to_string());
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
    println!("║   SEL Agent v1.7 — Stress Test           ║");
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
        ("go fizzbuzz",      "Create Go package main with FizzBuzz(n int) string returning Fizz/Buzz/FizzBuzz/number. Create go.mod module gotest go 1.21. Write _test.go with 4 test cases. Run go test."),
        ("go reverse",       "Create Go package main with Reverse(s string) string. Create go.mod module gotest go 1.21. Write _test.go testing Reverse(hello)==olleh and Reverse()==empty. Run go test."),
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
        pb.set_style(ProgressStyle::default_spinner()
            .template(&format!("{{spinner:.yellow}} ⏳ Running: {}...", name)).unwrap());
        pb.enable_steady_tick(Duration::from_millis(80));

        let mut ag = agent::Agent::new(
            api_key.to_string(),
            workspace.clone(),
            goal.to_string(),
            max_repairs,
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

    let avg = if passed > 0 { total_repairs as f64 / passed as f64 } else { 0.0 };
    println!("\n=== Stress Results: {}/{} passed | avg repairs: {:.1} ===\n", passed, total, avg);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Health => {
            let api_key = std::env::var("GROQ_API_KEY").expect("GROQ_API_KEY not set");
            run_health(&api_key).await?;
        }
        Commands::Bench { suite, max_repairs, iterations } => {
            let api_key = std::env::var("GROQ_API_KEY").expect("GROQ_API_KEY not set");
            run_bench(&api_key, &suite, max_repairs, iterations).await?;
        }
        Commands::Stress { max_repairs } => {
            let api_key = std::env::var("GROQ_API_KEY").expect("GROQ_API_KEY not set");
            run_stress(&api_key, max_repairs).await?;
        }
        Commands::Run { workspace, goal, max_repairs, dry_run } => {
            println!("\n╔══════════════════════════════════════════╗");
            println!("║   SEL Agent v1.5 — State Machine Engine  ║");
            println!("╚══════════════════════════════════════════╝");
            println!("\n📋 Goal: \"{}\"", goal);
            println!("   Workspace:   {}", workspace.display());
            println!("   Max repairs: {}", max_repairs);

            let api_key = std::env::var("GROQ_API_KEY").expect("GROQ_API_KEY not set");

            if dry_run {
                println!("   Mode:         🔍 DRY RUN\n");
                let llm = llm::LlmClient::new(api_key);
                let prompt = format!("Goal: {}\n\nProvide the complete execution plan.", goal);
                match llm.call(&[types::Message::user(prompt)]).await {
                    Ok(response) => match protocol::parse(&response) {
                        Ok(plan) => {
                            println!("📋 Plan preview ({} commands):\n", plan.commands.len());
                            for (i, cmd) in plan.commands.iter().enumerate() {
                                println!("  [{}/{}] {}", i+1, plan.commands.len(), cmd.label());
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
            let mut ag = agent::Agent::new(api_key, workspace, goal, max_repairs);
            ag.run().await?;
        }
    }
    Ok(())
}
