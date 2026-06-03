#!/usr/bin/env bash
set -euo pipefail

step() {
  echo
  echo "============================================================"
  echo "== $1"
  echo "============================================================"
}

check_build() {
  echo '$ cargo check'
  cargo check
  echo "✅ check passed"
}

step "0) backups"
for f in src/agent.rs src/scaffold_engine.rs src/repair_strategy.rs src/executor/runner.rs; do
  cp "$f" "$f.bak5"
  echo "backup: $f.bak5"
done

step "1) patch agent.rs: abort on Python/TypeScript scaffold failure"
python3 - <<'PYEOF'
from pathlib import Path

p = Path("src/agent.rs")
c = p.read_text()

old = '''        let scaffold =
            crate::scaffold_engine::prepare(&ws, &self.goal, self.llm.mode() == "replay").await;
        if scaffold.ready {
            println!(
                "   🏗  Scaffold ready: {:?} ({} files)",
                scaffold.kind,
                scaffold.files_created.len()
            );
            if !scaffold.logic_hint.is_empty() {
                self.goal = format!(
                    "{}
{}",
                    self.goal, scaffold.logic_hint
                );
            }
        }'''

new = '''        let scaffold =
            crate::scaffold_engine::prepare(&ws, &self.goal, self.llm.mode() == "replay").await;

        if !scaffold.ready
            && matches!(
                scaffold.kind,
                crate::scaffold_engine::ProjectKind::TypeScript
                    | crate::scaffold_engine::ProjectKind::Python
            )
        {
            let msg = if scaffold.logic_hint.is_empty() {
                format!("scaffold failed for {:?}", scaffold.kind)
            } else {
                format!("scaffold failed for {:?}: {}", scaffold.kind, scaffold.logic_hint)
            };
            eprintln!("   ❌ Scaffold failed: {}", msg);
            self.send_event("scaffold_failed", None, None, None, None);
            return Err(anyhow::anyhow!(msg));
        }

        if scaffold.ready {
            println!(
                "   🏗  Scaffold ready: {:?} ({} files)",
                scaffold.kind,
                scaffold.files_created.len()
            );
            if !scaffold.logic_hint.is_empty() {
                self.goal = format!(
                    "{}
{}",
                    self.goal, scaffold.logic_hint
                );
            }
        }'''

if old not in c:
    raise SystemExit("target block not found in src/agent.rs")

c = c.replace(old, new)
p.write_text(c)
print("✅ patched src/agent.rs")
PYEOF

check_build

step "2) patch scaffold_engine.rs: retry npm install 3 times with backoff"
python3 - <<'PYEOF'
from pathlib import Path

p = Path("src/scaffold_engine.rs")
c = p.read_text()

old = '''        let out = tokio::process::Command::new("npm")
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
                eprintln!(
                    "    TypeScript Scaffold FATAL: npm install failed with status {}",
                    o.status
                );
                eprintln!("    Details: {}", err.chars().take(200).collect::<String>());
                return ScaffoldResult {
                    kind: ProjectKind::TypeScript,
                    ready: false,
                    logic_hint: format!("npm install failed: {}", err),
                    files_created: created,
                };
            }
            Err(e) => {
                eprintln!("    TypeScript Scaffold FATAL: npm install failed: {}", e);
                eprintln!("    Fix: ensure node/npm are installed and workspace is writable");
                return ScaffoldResult {
                    kind: ProjectKind::TypeScript,
                    ready: false,
                    logic_hint: format!("npm install error: {}", e),
                    files_created: created,
                };
            }
        }'''

new = '''        let mut last_err = String::new();
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
        }'''

if old not in c:
    raise SystemExit("target npm block not found in src/scaffold_engine.rs")

c = c.replace(old, new)
p.write_text(c)
print("✅ patched src/scaffold_engine.rs")
PYEOF

check_build

step "3) patch runner.rs: do NOT treat '[no test files]' or '[no tests to run]' as success"
python3 - <<'PYEOF'
from pathlib import Path

p = Path("src/executor/runner.rs")
c = p.read_text()

old = '''            let no_test_files = combined.contains("[no test files]");
            let success = exit_ok && (passed > 0 || no_test_files);'''

new = '''            let no_test_files = combined.contains("[no test files]");
            let no_tests_to_run = combined.contains("[no tests to run]");
            let success = exit_ok && failed == 0 && passed > 0 && !no_test_files && !no_tests_to_run;'''

if old not in c:
    print("skip runner.rs: target not found (inspect manually if needed)")
else:
    c = c.replace(old, new)
    p.write_text(c)
    print("✅ patched src/executor/runner.rs")
PYEOF

check_build

step "4) patch repair_strategy.rs: explicit Constitution-aware prompt"
python3 - <<'PYEOF'
from pathlib import Path

p = Path("src/repair_strategy.rs")
c = p.read_text()

needle = '''pub fn build_prompt(attempt: u8, error: &str, ctx: &RepairCtx) -> String {
    if attempt > MAX_REPAIR_ATTEMPTS {
        return format!(
            "GIVING UP after {} attempts. Last error:\\n{}",
            attempt, error
        );
    }

    let is_loop = detect_error_loop(ctx, attempt);
'''

insert = '''pub fn build_prompt(attempt: u8, error: &str, ctx: &RepairCtx) -> String {
    if attempt > MAX_REPAIR_ATTEMPTS {
        return format!(
            "GIVING UP after {} attempts. Last error:\\n{}",
            attempt, error
        );
    }

    if error.contains("CONSTITUTION_VIOLATION:no-modify-tests") {
        return format!(
            "CRITICAL CONSTRAINT VIOLATION.\\n\
             You attempted to modify a protected test file.\\n\
             NEVER write or patch any test file: [{}].\\n\
             Fix SOURCE files ONLY: [{}].\\n\
             The tests define the contract and are immutable.\\n\
             Read the error carefully and change only implementation files.\\n\
             Error:\\n{}",
            ctx.test_files.join(", "),
            ctx.source_files.join(", "),
            error
        );
    }

    let is_loop = detect_error_loop(ctx, attempt);
'''

if needle not in c:
    raise SystemExit("target function prologue not found in src/repair_strategy.rs")

c = c.replace(needle, insert)
p.write_text(c)
print("✅ patched src/repair_strategy.rs")
PYEOF

check_build

step "5) format + validate"
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all --all-features
cargo build --release

step "6) inspection for next phase (protected tests model)"
echo
echo "--- file_ops head ---"
sed -n '1,90p' src/executor/file_ops.rs

echo
echo "--- types head ---"
sed -n '1,120p' src/types.rs

echo
echo "✅ phase5_scaffold_and_repair.sh completed"
