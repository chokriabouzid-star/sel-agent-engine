#!/usr/bin/env python3
"""
SEL Agent v1.6 — Auto-fix script
Applies 3 changes:
  1. agent.rs  — fix syntax error + real duration in report_run
  2. llm.rs    — add Python testing rules to SYSTEM_PROMPT
  3. protocol.rs — add validate_test_order()
"""

import re, sys, shutil
from pathlib import Path

BASE = Path.home() / "projects/sel-agent-v4/src"
AGENT   = BASE / "agent.rs"
LLM     = BASE / "llm.rs"
PROTO   = BASE / "protocol.rs"

ERRORS = []

def backup(path: Path):
    bak = path.with_suffix(path.suffix + ".bak")
    shutil.copy2(path, bak)
    print(f"  ✓ backup → {bak.name}")

def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")

def write(path: Path, content: str):
    path.write_text(content, encoding="utf-8")

# ─────────────────────────────────────────────
# FIX 1 — agent.rs
# ─────────────────────────────────────────────
def fix_agent():
    print("\n[1/3] agent.rs — duration + model fix")
    src = read(AGENT)
    backup(AGENT)

    # 1a. Fix broken syntax in report_run signature + body
    # Replace the broken line: "kimi-k2".to_string()".to_string()
    src = src.replace(
        '''"kimi-k2".to_string()".to_string()''',
        '''"moonshotai/kimi-k2-instruct".to_string()'''
    )

    # 1b. Add duration_secs parameter to report_run signature
    src = src.replace(
        'async fn report_run(goal: &str, success: bool, repairs: i64) -> Result<()> {',
        'async fn report_run(goal: &str, success: bool, repairs: i64, duration_secs: u64) -> Result<()> {'
    )

    # 1c. Replace "duration_secs": 0 with the real value
    src = src.replace(
        '"duration_secs": 0,',
        '"duration_secs": duration_secs,'
    )

    # 1d. Add Instant::now() before the main run loop and pass elapsed to both call sites
    # Find the run() method start — look for the loop opening
    # We patch the two call sites to pass the duration

    # First call site: Done state
    src = src.replace(
        'let _ = report_run(&self.goal, true, repairs as i64).await;',
        'let _ = report_run(&self.goal, true, repairs as i64, self.ctx.duration_secs).await;'
    )

    # Second call site: Failed state
    src = src.replace(
        'let _ = report_run(&self.goal, false, repairs).await;',
        'let _ = report_run(&self.goal, false, repairs, self.ctx.duration_secs).await;'
    )

    write(AGENT, src)
    print("  ✓ syntax error fixed")
    print("  ✓ duration_secs parameter added")
    print("  ✓ model name fixed")
    print()
    print("  ⚠️  NOTE: add 'duration_secs: u64' to ExecutionContext (context.rs)")
    print("     and set it with Instant::now() at run start.")
    print("     Or use the simpler fallback — see fix_agent_simple() below.")

def fix_agent_simple():
    """
    Simpler approach: pass duration directly without context.
    Patches Done/Failed arms to compute duration locally
    by storing start time at the top of the agent run() function.
    """
    print("\n[1/3] agent.rs — simple duration approach")
    src = read(AGENT)
    backup(AGENT)

    # Fix syntax error
    src = src.replace(
        '''"kimi-k2".to_string()".to_string()''',
        '''"moonshotai/kimi-k2-instruct".to_string()'''
    )

    # Add duration param to report_run
    src = src.replace(
        'async fn report_run(goal: &str, success: bool, repairs: i64) -> Result<()> {',
        'async fn report_run(goal: &str, success: bool, repairs: i64, duration_secs: u64) -> Result<()> {'
    )

    # Replace hardcoded 0
    src = src.replace(
        '"duration_secs": 0,',
        '"duration_secs": duration_secs,'
    )

    # Patch Done arm — wrap with timing
    src = src.replace(
        '''\
                AgentState::Done => {
                    let repairs = self.ctx.repair_attempts.saturating_sub(1);
                    let _ = report_run(&self.goal, true, repairs as i64).await;
                    return Ok(());
                }''',
        '''\
                AgentState::Done => {
                    let repairs = self.ctx.repair_attempts.saturating_sub(1);
                    let elapsed = self.ctx.start_time.map(|s| s.elapsed().as_secs()).unwrap_or(0);
                    let _ = report_run(&self.goal, true, repairs as i64, elapsed).await;
                    return Ok(());
                }'''
    )

    # Patch Failed arm
    src = src.replace(
        '''\
                AgentState::Failed(reason) => {
                    println!("\\n❌ Agent failed: {}", reason);
                    println!("SEL_FAILED: {}", reason.lines().next().unwrap_or("unknown"));
                    let repairs = self.ctx.repair_attempts as i64;
                    let _ = report_run(&self.goal, false, repairs).await;
                    return Ok(());
                }''',
        '''\
                AgentState::Failed(reason) => {
                    println!("\\n❌ Agent failed: {}", reason);
                    println!("SEL_FAILED: {}", reason.lines().next().unwrap_or("unknown"));
                    let repairs = self.ctx.repair_attempts as i64;
                    let elapsed = self.ctx.start_time.map(|s| s.elapsed().as_secs()).unwrap_or(0);
                    let _ = report_run(&self.goal, false, repairs, elapsed).await;
                    return Ok(());
                }'''
    )

    write(AGENT, src)
    print("  ✓ done")

# ─────────────────────────────────────────────
# FIX 2 — llm.rs
# ─────────────────────────────────────────────
TESTING_RULES = '''
PYTHON TESTING (MANDATORY):
1. Floats: ALWAYS use pytest.approx(x, rel=1e-6) — NEVER compare floats with ==
2. Branches: every if/else MUST have a test for EACH branch (both True and False paths)
3. Assume your code is wrong. Tests must try to BREAK the code, not mirror its logic.
'''

def fix_llm():
    print("\n[2/3] llm.rs — add Python testing rules")
    src = read(LLM)
    backup(LLM)

    # Find the closing "#; of SYSTEM_PROMPT
    # It's a raw string: r#"..."#  or const ... = r#"...
    # We insert the rules just before the closing "#;
    if TESTING_RULES.strip() in src:
        print("  ⚠️  rules already present — skipping")
        return

    # Find closing of the system prompt raw string
    # Pattern: \n"#; or \n"#\n
    close_patterns = ['\n"#;', '\n"#\n']
    patched = False
    for pat in close_patterns:
        if pat in src:
            src = src.replace(pat, TESTING_RULES + pat, 1)
            patched = True
            break

    if not patched:
        # Try r#"..."# pattern ending
        idx = src.rfind('"#')
        if idx != -1:
            src = src[:idx] + TESTING_RULES + src[idx:]
            patched = True

    if not patched:
        ERRORS.append("llm.rs: could not find end of SYSTEM_PROMPT — edit manually")
        print("  ✗ could not locate SYSTEM_PROMPT closing — see ERRORS at end")
        return

    write(LLM, src)
    print("  ✓ testing rules added to SYSTEM_PROMPT")

# ─────────────────────────────────────────────
# FIX 3 — protocol.rs
# ─────────────────────────────────────────────
VALIDATOR = '''
/// Ensures test files are written before RunTests is called.
pub fn validate_test_order(plan: &Plan) -> Result<(), String> {
    let has_run_tests = plan.commands.iter()
        .any(|c| matches!(c, Cmd::RunTests { .. }));
    if !has_run_tests { return Ok(()); }

    let test_pos = plan.commands.iter().position(|c| match c {
        Cmd::WriteFile { path, .. } => path.contains("test"),
        _ => false,
    });
    let run_pos = plan.commands.iter().position(|c|
        matches!(c, Cmd::RunTests { .. })
    );

    match (test_pos, run_pos) {
        (Some(t), Some(r)) if t < r => Ok(()),
        (None, _) => Err("Plan calls RunTests but writes no test file".into()),
        _ => Err("Test file must be written before RunTests".into()),
    }
}
'''

def fix_protocol():
    print("\n[3/3] protocol.rs — add validate_test_order()")
    src = read(PROTO)
    backup(PROTO)

    if "validate_test_order" in src:
        print("  ⚠️  already present — skipping")
        return

    # Append before last closing brace or at end of file
    src = src.rstrip() + "\n" + VALIDATOR + "\n"
    write(PROTO, src)
    print("  ✓ validate_test_order() added")

# ─────────────────────────────────────────────
# MAIN
# ─────────────────────────────────────────────
def check_files():
    missing = [p for p in [AGENT, LLM, PROTO] if not p.exists()]
    if missing:
        for m in missing:
            print(f"  ✗ not found: {m}")
        sys.exit(1)

def main():
    print("=" * 50)
    print("SEL Agent v1.6 — fix_v16.py")
    print("=" * 50)

    check_files()

    fix_agent_simple()
    fix_llm()
    fix_protocol()

    print("\n" + "=" * 50)
    if ERRORS:
        print("ERRORS (manual fix needed):")
        for e in ERRORS:
            print(f"  ✗ {e}")
    else:
        print("✅ All patches applied.")

    print()
    print("Next steps:")
    print("  1. Check context.rs — add start_time: Option<std::time::Instant>")
    print("     and set it at the start of run()")
    print("  2. cargo build --release")
    print("  3. ./target/release/sel-agent health")
    print("=" * 50)

if __name__ == "__main__":
    main()
