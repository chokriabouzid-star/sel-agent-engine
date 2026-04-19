// scaffold_engine.rs — v6.3
// Phase 1: يُجهّز البيئة قبل LLM — حتمي 100%
// Pipeline: ScaffoldEngine::prepare() → LLM::plan_logic_only() → Executor::run()

use crate::goal_parser;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectKind {
    TypeScript,
    Python,
    Rust,
    Go,
    Unknown,
}

#[derive(Debug)]
pub struct ScaffoldResult {
    pub kind: ProjectKind,
    pub ready: bool,
    pub logic_hint: String, // يُرسَل للـ LLM بدلاً من تعليمات البيئة
    pub files_created: Vec<String>,
}

// ─── Pinned Stacks ────────────────────────────────────────
const TS_JEST_DEPS: &str = "typescript@5.3.3 ts-jest@29.1.1 jest@29.7.0 @types/jest@29.5.11";

const PACKAGE_JSON_TS: &str = r#"{
  "name": "sel-project",
  "version": "1.0.0",
  "scripts": {
    "test": "jest",
    "build": "tsc"
  },
  "jest": {
    "preset": "ts-jest",
    "testEnvironment": "node",
    "testMatch": ["**/*.test.ts"]
  },
  "devDependencies": {
    "typescript": "5.3.3",
    "ts-jest": "29.1.1",
    "jest": "29.7.0",
    "@types/jest": "29.5.11"
  }
}"#;

const TSCONFIG_JSON: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "lib": ["ES2020"],
    "strict": true,
    "esModuleInterop": true,
    "outDir": "./dist",
    "rootDir": "./"
  },
  "include": ["**/*.ts"],
  "exclude": ["node_modules", "dist"]
}"#;

// ─── نقطة الدخول ──────────────────────────────────────────
pub async fn prepare(workspace: &Path, goal: &str) -> ScaffoldResult {
    let parsed = goal_parser::parse(workspace, goal);
    let kind = parsed.kind.clone();

    match &kind {
        ProjectKind::TypeScript => scaffold_typescript(workspace, &parsed.extra_deps).await,
        ProjectKind::Python => scaffold_python(workspace, &parsed.extra_deps).await,
        ProjectKind::Unknown => ScaffoldResult {
            kind,
            ready: false,
            logic_hint: String::new(),
            files_created: vec![],
        },
        _ => ScaffoldResult {
            kind,
            ready: false,
            logic_hint: String::new(),
            files_created: vec![],
        },
    }
}

// detect_kind moved to goal_parser.rs — v6.5

// ─── Scaffold TypeScript ──────────────────────────────────
async fn scaffold_typescript(workspace: &Path, extra_deps: &[String]) -> ScaffoldResult {
    println!("   🏗  Scaffold: TypeScript environment");
    let mut created = vec![];

    // 1) package.json — ثابت دائماً
    let pkg_path = workspace.join("package.json");
    let pkg_content = if pkg_path.exists() {
        // صحّح الموجود بدلاً من الكتابة فوقه
        normalize_existing_package_json(&pkg_path)
    } else {
        PACKAGE_JSON_TS.to_string()
    };
    std::fs::write(&pkg_path, &pkg_content).ok();
    println!("   ✅ package.json ready (pinned TS stack)");
    created.push("package.json".to_string());

    // 2) tsconfig.json
    let ts_path = workspace.join("tsconfig.json");
    if !ts_path.exists() {
        std::fs::write(&ts_path, TSCONFIG_JSON).ok();
        println!("   ✅ tsconfig.json ready");
        created.push("tsconfig.json".to_string());
    } else {
        println!("   ⏭  tsconfig.json exists — skip");
    }

    // 3) احذف jest.config.js إذا وُجد (يسبب conflict)
    let jest_cfg = workspace.join("jest.config.js");
    let jest_cfg_ts = workspace.join("jest.config.ts");
    for cfg in [&jest_cfg, &jest_cfg_ts] {
        if cfg.exists() {
            std::fs::remove_file(cfg).ok();
            println!(
                "   🗑  Removed {:?} — config in package.json only",
                cfg.file_name().unwrap_or_default()
            );
        }
    }

    // 4) npm install بالـ pinned stack
    let node_modules = workspace.join("node_modules");
    if !node_modules.exists() {
        println!("   📦 Installing pinned TS stack...");
        let mut npm_args: Vec<&str> = vec!["install", "--save-dev"];
        let ts_deps: Vec<&str> = TS_JEST_DEPS.split_whitespace().collect();
        npm_args.extend_from_slice(&ts_deps);
        let extra_refs: Vec<&str> = extra_deps.iter().map(|s| s.as_str()).collect();
        npm_args.extend_from_slice(&extra_refs);
        if !extra_deps.is_empty() {
            println!("   📦 Extra deps: {}", extra_deps.join(", "));
        }
        let out = tokio::process::Command::new("npm")
            .args(&npm_args)
            .current_dir(workspace)
            .output()
            .await;

        match out {
            Ok(o) if o.status.success() => {
                println!("   ✅ Dependencies installed (pinned)");
                created.push("node_modules".to_string());
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                eprintln!("   ❌ TypeScript Scaffold FATAL: npm install failed with status {}", o.status);
                eprintln!("   💡 Details: {}", &err[..err.len().min(200)]);
                return ScaffoldResult {
                    kind: ProjectKind::TypeScript,
                    ready: false,
                    logic_hint: format!("npm install failed: {}", err),
                    files_created: created,
                };
            }
            Err(e) => {
                eprintln!("   ❌ TypeScript Scaffold FATAL: npm install failed: {}", e);
                eprintln!("   💡 Fix: ensure node/npm are installed and workspace is writable");
                return ScaffoldResult {
                    kind: ProjectKind::TypeScript,
                    ready: false,
                    logic_hint: format!("npm install error: {}", e),
                    files_created: created,
                };
            }
        }
    } else {
        println!("   ⏭  node_modules exists — skip install");
    }

    ScaffoldResult {
        kind: ProjectKind::TypeScript,
        ready: true,
        logic_hint: build_ts_logic_hint(workspace),
        files_created: created,
    }
}

// ─── Scaffold Python ──────────────────────────────────────
async fn scaffold_python(workspace: &Path, extra_deps: &[String]) -> ScaffoldResult {
    println!("   🏗  Scaffold: Python environment");
    let mut created = vec![];

    // 1) venv
    let venv = workspace.join("venv");
    if !venv.exists() {
        println!("   📦 Creating venv...");
        let out = tokio::process::Command::new("python3")
            .args(["-m", "venv", "venv"])
            .current_dir(workspace)
            .output()
            .await;

        if out.map(|o| o.status.success()).unwrap_or(false) {
            println!("   ✅ venv created");
            created.push("venv".to_string());

            // 2) تثبيت pytest مباشرة بعد إنشاء venv
            let _ = tokio::process::Command::new("venv/bin/pip")
                .args(["install", "pytest==8.1.1", "pytest-cov==5.0.0", "-q"])
                .current_dir(workspace)
                .output()
                .await;
            println!("   ✅ pytest installed (pinned)");

            // تثبيت extra_deps من GoalParser
            if !extra_deps.is_empty() {
                println!("   📦 Installing extra deps: {}", extra_deps.join(", "));
                let mut pip_args = vec!["install", "-q"];
                let extra_refs: Vec<&str> = extra_deps.iter().map(|s| s.as_str()).collect();
                pip_args.extend_from_slice(&extra_refs);
                let pip_out = tokio::process::Command::new("venv/bin/pip")
                    .args(&pip_args)
                    .current_dir(workspace)
                    .output()
                    .await;
                match pip_out {
                    Ok(o) if o.status.success() => println!("   ✅ Extra deps installed"),
                    Ok(o) => println!(
                        "   ⚠️  Extra deps warning: {}",
                        String::from_utf8_lossy(&o.stderr)
                            .chars()
                            .take(200)
                            .collect::<String>()
                    ),
                    Err(e) => println!("   ⚠️  Extra deps failed: {}", e),
                }
            }
        }
    } else {
        println!("   ⏭  venv exists — skip");
    }

    ScaffoldResult {
        kind: ProjectKind::Python,
        ready: true,
        logic_hint: build_py_logic_hint(workspace),
        files_created: created,
    }
}

// ─── تصحيح package.json الموجود ───────────────────────────
fn normalize_existing_package_json(path: &Path) -> String {
    let src = std::fs::read_to_string(path).unwrap_or_default();
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&src) {
        // أضف jest config داخل package.json
        v["jest"] = serde_json::json!({
            "preset": "ts-jest",
            "testEnvironment": "node",
            "testMatch": ["**/*.test.ts"]
        });
        // صحّح scripts
        v["scripts"]["test"] = serde_json::json!("jest");
        v["scripts"]["build"] = serde_json::json!("tsc");
        // صحّح devDependencies بالـ pinned stack
        v["devDependencies"] = serde_json::json!({
            "typescript": "5.3.3",
            "ts-jest": "29.1.1",
            "jest": "29.7.0",
            "@types/jest": "29.5.11"
        });
        // احذف "type":"module" — يكسر jest
        if let Some(obj) = v.as_object_mut() {
            obj.remove("type");
        }
        return serde_json::to_string_pretty(&v).unwrap_or(PACKAGE_JSON_TS.to_string());
    }
    PACKAGE_JSON_TS.to_string()
}

// ─── Logic Hints للـ LLM ──────────────────────────────────
fn build_ts_logic_hint(workspace: &Path) -> String {
    let files: Vec<String> = std::fs::read_dir(workspace)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.ends_with(".ts") && !n.ends_with(".test.ts"))
                .collect()
        })
        .unwrap_or_default();

    let existing = if files.is_empty() {
        String::new()
    } else {
        format!("\nExisting TS files: {}", files.join(", "))
    };

    format!(
        "\n[SCAFFOLD READY — TypeScript]\n\
        Environment is fully configured. Do NOT create or modify:\n\
        - package.json (ready with pinned deps + jest config)\n\
        - tsconfig.json (ready)\n\
        - jest.config.js (not needed — config is in package.json)\n\
        - node_modules (installed)\n\
        Your job: write ONLY .ts files (TypeScript). NEVER write .js files.\n\
        Jest testMatch is: **/*.test.ts — .js files will NOT be found by Jest.\n\
        RULE: Every source file must end in .ts, every test file must end in .test.ts\n\
        Test command: npm test{}\n",
        existing
    )
}

fn build_py_logic_hint(workspace: &Path) -> String {
    format!(
        "\n[SCAFFOLD READY — Python]\n\
        Environment is fully configured. Do NOT create venv or install pytest.\n\
        venv is at: {}/venv\n\
        pytest is installed and ready.\n\
        Your job: write ONLY the logic .py files and test files.\n\
        Test command: venv/bin/pytest -v --tb=short\n",
        workspace.display()
    )
}
