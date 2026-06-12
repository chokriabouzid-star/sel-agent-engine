// scaffold_engine.rs  v6.3
// Phase 1:    LLM   100%
// Pipeline: ScaffoldEngine::prepare()  LLM::plan_logic_only()  Executor::run()

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
    pub logic_hint: String, //   LLM
    pub files_created: Vec<String>,
}

//  Pinned Stacks
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

// v8.4.2: axios pinned version for TypeScript HTTP client tasks
const AXIOS_VERSION: &str = "1.6.7";

const TSCONFIG_JSON: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "lib": ["ES2020"],
    "strict": true,
    "esModuleInterop": true,
    "outDir": "./dist",
    "rootDir": "./",
    "types": ["jest", "node"]
  },
  "include": ["**/*.ts"],
  "exclude": ["node_modules", "dist"]
}"#;

//
pub async fn prepare(workspace: &Path, goal: &str, replay_mode: bool) -> ScaffoldResult {
    if replay_mode {
        return prepare_from_cache(workspace, goal).await;
    }

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

pub fn get_cache_dir() -> std::path::PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.cache"))
        .join("sel-agent/scaffold")
}

async fn prepare_from_cache(workspace: &Path, goal: &str) -> ScaffoldResult {
    let cache_dir = get_cache_dir();
    let parsed = goal_parser::parse(workspace, goal);

    match &parsed.kind {
        ProjectKind::TypeScript => {
            let pkg_path = workspace.join("package.json");
            let pkg_content = if pkg_path.exists() {
                normalize_existing_package_json(&pkg_path)
            } else {
                PACKAGE_JSON_TS.to_string()
            };
            std::fs::write(&pkg_path, &pkg_content).ok();

            let ts_path = workspace.join("tsconfig.json");
            if !ts_path.exists() {
                std::fs::write(&ts_path, TSCONFIG_JSON).ok();
            }

            let jest_cfg = workspace.join("jest.config.js");
            let jest_cfg_ts = workspace.join("jest.config.ts");
            std::fs::remove_file(jest_cfg).ok();
            std::fs::remove_file(jest_cfg_ts).ok();

            let cached_nm = cache_dir.join("node/node_modules");
            if cached_nm.exists() {
                let target = workspace.join("node_modules");
                if !target.exists() {
                    let _ = std::os::unix::fs::symlink(&cached_nm, &target);
                    println!("   ⚡ Scaffold cache hit: node_modules symlinked");
                }
                let mut created = vec!["package.json".to_string(), "node_modules".to_string()];
                if ts_path.exists() {
                    created.push("tsconfig.json".to_string());
                }
                if !parsed.extra_deps.is_empty() {
                    // v7.5.7: Offline Replay Check - only install if missing
                    let mut missing_deps = vec![];
                    for dep in &parsed.extra_deps {
                        let pkg_name = if dep.starts_with('@') {
                            dep.clone()
                        } else {
                            dep.split('@').next().unwrap_or(dep).to_string()
                        };
                        if !workspace.join("node_modules").join(&pkg_name).exists() {
                            missing_deps.push(dep.clone());
                        }
                    }

                    if !missing_deps.is_empty() {
                        println!(
                            "    Cache hit: installing missing extra deps: {}",
                            missing_deps.join(", ")
                        );
                        let mut args = vec!["install", "--no-save"];
                        let extra_refs: Vec<&str> =
                            missing_deps.iter().map(|s| s.as_str()).collect();
                        args.extend(extra_refs);
                        let _ = tokio::process::Command::new("npm")
                            .args(&args)
                            .current_dir(workspace)
                            .output()
                            .await;
                    }
                }
                return ScaffoldResult {
                    kind: ProjectKind::TypeScript,
                    ready: true,
                    logic_hint: build_ts_logic_hint(workspace),
                    files_created: created,
                };
            }
            let res = scaffold_typescript(workspace, &parsed.extra_deps).await;
            if workspace.join("node_modules").exists() && res.ready {
                if let Some(parent) = cached_nm.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = tokio::process::Command::new("rm")
                    .args(["-rf", cached_nm.to_str().unwrap_or_default()])
                    .output()
                    .await;
                let _ = tokio::process::Command::new("cp")
                    .args(["-R", "node_modules", cached_nm.to_str().unwrap_or_default()])
                    .current_dir(workspace)
                    .output()
                    .await;
            }
            res
        }
        ProjectKind::Python => {
            let cached_venv = cache_dir.join("python/venv");
            if cached_venv.exists() && cached_venv.join("bin/pytest").exists() {
                let target = workspace.join("venv");
                if !target.exists() {
                    let _ = std::os::unix::fs::symlink(&cached_venv, &target);
                    println!("   ⚡ Scaffold cache hit: venv symlinked");
                }
                if !parsed.extra_deps.is_empty() {
                    // v7.5.7: Offline Replay Check - only install if missing
                    let mut missing_deps = vec![];
                    for dep in &parsed.extra_deps {
                        let module_name = match dep.as_str() {
                            "fastapi" => "fastapi",
                            "uvicorn[standard]" => "uvicorn",
                            "flask" => "flask",
                            "httpx" => "httpx",
                            "requests" => "requests",
                            _ => dep.as_str(),
                        };
                        let out = tokio::process::Command::new("venv/bin/python3")
                            .args(["-c", &format!("import {}", module_name)])
                            .current_dir(workspace)
                            .output()
                            .await;

                        if !out.map(|o| o.status.success()).unwrap_or(false) {
                            missing_deps.push(dep.clone());
                        }
                    }

                    if !missing_deps.is_empty() {
                        println!(
                            "    Cache hit: installing missing extra deps: {}",
                            missing_deps.join(", ")
                        );
                        let mut pip_args = vec!["install", "-q"];
                        let extra_refs: Vec<&str> =
                            missing_deps.iter().map(|s| s.as_str()).collect();
                        pip_args.extend(extra_refs);
                        let _ = tokio::process::Command::new("venv/bin/pip")
                            .args(&pip_args)
                            .current_dir(workspace)
                            .output()
                            .await;
                    }
                }
                return ScaffoldResult {
                    kind: ProjectKind::Python,
                    ready: true,
                    logic_hint: build_py_logic_hint(workspace),
                    files_created: vec!["venv".to_string()],
                };
            }
            let res = scaffold_python(workspace, &parsed.extra_deps).await;
            if workspace.join("venv").exists() && res.ready {
                if let Some(parent) = cached_venv.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = tokio::process::Command::new("rm")
                    .args(["-rf", cached_venv.to_str().unwrap_or_default()])
                    .output()
                    .await;
                let _ = tokio::process::Command::new("cp")
                    .args(["-R", "venv", cached_venv.to_str().unwrap_or_default()])
                    .current_dir(workspace)
                    .output()
                    .await;
            }
            res
        }
        _ => {
            // For other types, or if Unknown, fallback to normal prepare
            let kind = parsed.kind.clone();
            match &kind {
                ProjectKind::TypeScript => scaffold_typescript(workspace, &parsed.extra_deps).await,
                ProjectKind::Python => scaffold_python(workspace, &parsed.extra_deps).await,
                _ => ScaffoldResult {
                    kind,
                    ready: false,
                    logic_hint: String::new(),
                    files_created: vec![],
                },
            }
        }
    }
}

// detect_kind moved to goal_parser.rs  v6.5

//  Scaffold TypeScript
async fn scaffold_typescript(workspace: &Path, extra_deps: &[String]) -> ScaffoldResult {
    println!("   🏗  Scaffold: TypeScript environment");
    let mut created = vec![];

    // 1) package.json
    let pkg_path = workspace.join("package.json");
    let pkg_content = if pkg_path.exists() {
        //
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
        println!("   ⏭  tsconfig.json exists  skip");
    }

    // 3)  jest.config.js   ( conflict)
    let jest_cfg = workspace.join("jest.config.js");
    let jest_cfg_ts = workspace.join("jest.config.ts");
    for cfg in [&jest_cfg, &jest_cfg_ts] {
        if cfg.exists() {
            std::fs::remove_file(cfg).ok();
            println!(
                "     Removed {:?}  config in package.json only",
                cfg.file_name().unwrap_or_default()
            );
        }
    }

    // 4) npm install  pinned stack
    let node_modules = workspace.join("node_modules");
    if !node_modules.exists() {
        println!("   📦 Installing pinned TS stack...");
        let mut npm_args: Vec<&str> = vec!["install", "--save-dev"];
        let ts_deps: Vec<&str> = TS_JEST_DEPS.split_whitespace().collect();
        npm_args.extend_from_slice(&ts_deps);

        // v7.5.8: Pre-install common benchmark deps so the cache is fully offline-capable
        let common_bench_deps = ["express", "@types/express", "supertest", "@types/supertest"];
        npm_args.extend_from_slice(&common_bench_deps);

        let extra_refs: Vec<&str> = extra_deps.iter().map(|s| s.as_str()).collect();
        npm_args.extend_from_slice(&extra_refs);
        if !extra_deps.is_empty() {
            println!("   📦 Extra deps: {}", extra_deps.join(", "));
        }
        let mut last_err = String::new();
        let mut installed = false;

        for attempt in 1..=3 {
            if attempt > 1 {
                println!("   🔁 npm install retry {}/3...", attempt);
                tokio::time::sleep(tokio::time::Duration::from_secs(attempt as u64)).await;
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
                    installed = true;
                    break;
                }
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr);
                    let out_s = String::from_utf8_lossy(&o.stdout);
                    last_err = format!(
                        "npm install failed with status {} | stderr: {} | stdout: {}",
                        o.status,
                        err.chars().take(300).collect::<String>(),
                        out_s.chars().take(200).collect::<String>()
                    );
                    eprintln!(
                        "    TypeScript Scaffold WARN: npm install attempt {}/3 failed with status {}",
                        attempt,
                        o.status
                    );
                    eprintln!("    Details: {}", err.chars().take(200).collect::<String>());
                }
                Err(e) => {
                    last_err = format!("npm install error: {}", e);
                    eprintln!(
                        "    TypeScript Scaffold WARN: npm install attempt {}/3 errored: {}",
                        attempt, e
                    );
                }
            }
        }

        if !installed {
            eprintln!("    TypeScript Scaffold FATAL: npm install failed after retries");
            return ScaffoldResult {
                kind: ProjectKind::TypeScript,
                ready: false,
                logic_hint: last_err,
                files_created: created,
            };
        }
    } else {
        println!("   ⏭  node_modules exists  skip install");
    }

    ScaffoldResult {
        kind: ProjectKind::TypeScript,
        ready: true,
        logic_hint: build_ts_logic_hint(workspace),
        files_created: created,
    }
}

//  Scaffold Python
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

            // 2)  pytest    venv (      )
            let pip_out = tokio::process::Command::new("venv/bin/pip")
                .args([
                    "install",
                    "pytest==8.1.1",
                    "pytest-cov==5.0.0",
                    "flask",
                    "fastapi",
                    "uvicorn[standard]",
                    "httpx",
                    "requests",
                    "-q",
                ])
                .current_dir(workspace)
                .output()
                .await;

            match pip_out {
                Ok(o) if o.status.success() => {
                    println!("   ✅ pytest & common benchmark deps installed (pinned)");
                }
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr);
                    eprintln!(
                        "    Python Scaffold FATAL: pip install failed with status {}",
                        o.status
                    );
                    eprintln!("    Details: {}", err.chars().take(200).collect::<String>());
                    return ScaffoldResult {
                        kind: ProjectKind::Python,
                        ready: false,
                        logic_hint: format!("pip install failed: {}", err),
                        files_created: created,
                    };
                }
                Err(e) => {
                    eprintln!("    Python Scaffold FATAL: pip install failed: {}", e);
                    return ScaffoldResult {
                        kind: ProjectKind::Python,
                        ready: false,
                        logic_hint: format!("pip install error: {}", e),
                        files_created: created,
                    };
                }
            }

            //  extra_deps  GoalParser
            if !extra_deps.is_empty() {
                println!("   📦 Installing extra deps: {}", extra_deps.join(", "));
                let mut pip_args = vec!["install", "-q"];
                let extra_refs: Vec<&str> = extra_deps.iter().map(|s| s.as_str()).collect();
                pip_args.extend_from_slice(&extra_refs);
                let pip_out2 = tokio::process::Command::new("venv/bin/pip")
                    .args(&pip_args)
                    .current_dir(workspace)
                    .output()
                    .await;
                match pip_out2 {
                    Ok(o) if o.status.success() => println!("   ✅ Extra deps installed"),
                    Ok(o) => println!(
                        "     Extra deps warning: {}",
                        String::from_utf8_lossy(&o.stderr)
                            .chars()
                            .take(200)
                            .collect::<String>()
                    ),
                    Err(e) => println!("   ❌ Extra deps failed: {}", e),
                }
            }
        }
    } else {
        println!("   ⏭  venv exists  skip");
    }

    ScaffoldResult {
        kind: ProjectKind::Python,
        ready: true,
        logic_hint: build_py_logic_hint(workspace),
        files_created: created,
    }
}

//   package.json
fn normalize_existing_package_json(path: &Path) -> String {
    let src = std::fs::read_to_string(path).unwrap_or_default();
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&src) {
        //  jest config  package.json
        v["jest"] = serde_json::json!({
            "preset": "ts-jest",
            "testEnvironment": "node",
            "testMatch": ["**/*.test.ts"]
        });
        //  scripts
        v["scripts"]["test"] = serde_json::json!("jest");
        v["scripts"]["build"] = serde_json::json!("tsc");
        //  devDependencies  pinned stack
        v["devDependencies"] = serde_json::json!({
            "typescript": "5.3.3",
            "ts-jest": "29.1.1",
            "jest": "29.7.0",
            "@types/jest": "29.5.11"
        });
        //  "type":"module"   jest
        if let Some(obj) = v.as_object_mut() {
            obj.remove("type");
        }
        return serde_json::to_string_pretty(&v).unwrap_or(PACKAGE_JSON_TS.to_string());
    }
    PACKAGE_JSON_TS.to_string()
}

//  Logic Hints  LLM
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
        "\n[SCAFFOLD READY  TypeScript]\n\
        Environment is fully configured. Do NOT create or modify:\n\
        - package.json (ready with pinned deps + jest config)\n\
        - tsconfig.json (ready)\n\
        - jest.config.js (not needed  config is in package.json)\n\
        - node_modules (installed)\n\
        Your job: write ONLY .ts files (TypeScript). NEVER write .js files.\n\
        Jest testMatch is: **/*.test.ts  .js files will NOT be found by Jest.\n\
        RULE: Every source file must end in .ts, every test file must end in .test.ts\n\
        Test command: npm test{}\n",
        existing
    )
}

fn build_py_logic_hint(workspace: &Path) -> String {
    format!(
        "\n[SCAFFOLD READY  Python]\n\
        Environment is fully configured. Do NOT create venv or install pytest.\n\
        venv is at: {}/venv\n\
        pytest is installed and ready.\n\
        Your job: write ONLY the logic .py files and test files.\n\
        Test command: venv/bin/pytest -v --tb=short\n",
        workspace.display()
    )
}
