#!/usr/bin/env bash
set -euo pipefail

echo "============================================================"
echo "== Applying Root Cause Fixes"
echo "============================================================"

python3 - <<'PYEOF'
import re
from pathlib import Path

# ---------------------------------------------------------
# 1. parsers.rs (Go Oracle Strictness)
# ---------------------------------------------------------
p = Path("src/executor/parsers.rs")
if p.exists():
    c = p.read_text()
    if "[no test files]" not in c:
        c = c.replace(
            "pub fn parse_go_test_output(output: &str) -> (u32, u32, u32) {",
            """pub fn parse_go_test_output(output: &str) -> (u32, u32, u32) {
    if output.contains("[no test files]") || output.contains("[no tests to run]") {
        return (0, 1, 0); // 🚨 Force failure if 0 tests ran
    }"""
        )
        p.write_text(c)
        print("✅ parsers.rs patched (Go Oracle strictness)")

# ---------------------------------------------------------
# 2. file_ops.rs (Constitution Context Awareness)
# ---------------------------------------------------------
p = Path("src/executor/file_ops.rs")
if p.exists():
    c = p.read_text()
    # Apply path.exists() to check_write calls
    c_new = re.sub(
        r'constitution::check_write\(([^,]+),\s*([^,]+),\s*([^)]+)\)',
        r'constitution::check_write(\1, \2, (\3) && \1.exists())',
        c
    )
    if c != c_new:
        p.write_text(c_new)
        print("✅ file_ops.rs patched (path.exists() injected)")

# ---------------------------------------------------------
# 3. constitution.rs (LLM Prompt Amplification)
# ---------------------------------------------------------
p = Path("src/constitution.rs")
if p.exists():
    c = p.read_text()
    old_fmt = '''write!(
            f,
            "Constitution Rule {} [{}] violated: {}",
            self.rule_id, self.rule_name, self.detail
        )'''
    new_fmt = '''write!(
            f,
            "CONSTITUTION_VIOLATION:{}\\nCRITICAL INSTRUCTION: You attempted to violate a hard constraint. The test contract is fixed and CANNOT be modified. You MUST fix the SOURCE code ONLY. Do NOT output write_file for tests.\\nDetail: {}",
            self.rule_name, self.detail
        )'''
    if old_fmt in c:
        c = c.replace(old_fmt, new_fmt)
        p.write_text(c)
        print("✅ constitution.rs patched (LLM prompt amplification)")

# ---------------------------------------------------------
# 4. bench_swe.rs (TS-01 broken syntax removal)
# ---------------------------------------------------------
p = Path("src/bench_swe.rs")
if p.exists():
    c = p.read_text()
    # Remove the specific broken mkdb line
    c_new = re.sub(r'const mkdb = \(\) => \(\{.*?\}\);\\n\s*', '', c)
    if c != c_new:
        p.write_text(c_new)
        print("✅ bench_swe.rs patched (TS-01 broken syntax removed)")

PYEOF

echo
echo "============================================================"
echo "== Compiling and Verifying"
echo "============================================================"
cargo check
cargo test --all --all-features
cargo build --release

echo
echo "============================================================"
echo "== Cleaning up broken trajectories"
echo "============================================================"
rm -rf fixtures/trajectories/swe_go_01/ 2>/dev/null || true
rm -rf fixtures/trajectories/swe_go_03/ 2>/dev/null || true
rm -rf fixtures/trajectories/swe_ts_01/ 2>/dev/null || true
rm -rf fixtures/trajectories/smoke_py_binary_search/ 2>/dev/null || true
rm -rf fixtures/trajectories/smoke_ts_retry/ 2>/dev/null || true
rm -rf fixtures/trajectories/smoke_ts_fastapi_client/ 2>/dev/null || true
rm -rf fixtures/trajectories/smoke_go_worker_pool/ 2>/dev/null || true
rm -rf fixtures/trajectories/smoke_go_concurrent/ 2>/dev/null || true

echo
echo "✅ All root causes fixed and compiled successfully!"
echo "------------------------------------------------------------"
echo "👉 NEXT STEPS — Run these directly in your terminal:"
echo "------------------------------------------------------------"
echo "./target/release/sel-agent bench-swe --lang go --focus GO-01,GO-03 --record"
echo "./target/release/sel-agent bench-swe --lang typescript --focus TS-01 --record"
echo "bash sel_smoke_test.sh ./target/release/sel-agent --record"

