#!/usr/bin/env bash
# SEL Agent v8.5 — Pre-Release Health Check
# Usage:
#   bash sel_healthcheck.sh 2>&1 | tee /tmp/healthcheck_v850.txt
#   HEALTHCHECK_RERECORD=1 bash sel_healthcheck.sh 2>&1 | tee /tmp/healthcheck_v850_rerecord.txt

AGENT=./target/release/sel-agent
PASS=0
FAIL=0
WARN=0

green() { echo -e "\033[32m✅ $*\033[0m"; PASS=$((PASS+1)); }
red()   { echo -e "\033[31m❌ $*\033[0m"; FAIL=$((FAIL+1)); }
yellow(){ echo -e "\033[33m⚠️  $*\033[0m"; WARN=$((WARN+1)); }
hdr()   { echo -e "\n\033[1;34m══ $* ══\033[0m"; }

RERECORD_FLAG=""
MODE_LABEL="replay only"
if [ "${HEALTHCHECK_RERECORD:-0}" = "1" ]; then
  RERECORD_FLAG="--rerecord"
  MODE_LABEL="replay + rerecord"
fi

# ─── 1. Build Environment ─────────────────────────────
hdr "1. Build Environment"
rustc --version 2>/dev/null && green "rustc OK" || red "rustc not found"
cargo --version 2>/dev/null && green "cargo OK" || red "cargo not found"

if cargo check --quiet; then
  green "cargo check OK"
else
  red "cargo check failed"
fi

# ─── 2. Version Consistency ───────────────────────────
hdr "2. Version Consistency"
V_CARGO=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
V_MAIN=$(grep -o 'SEL Agent v[0-9.]*' src/main.rs 2>/dev/null | head -1 | sed 's/SEL Agent v//')
V_CLI=$(grep -Eo 'version *= *"[0-9.]+"' src/commands/cli.rs 2>/dev/null | head -1 | sed -E 's/.*"([0-9.]+)".*/\1/')

[ -z "$V_MAIN" ] && V_MAIN="N/A"
[ -z "$V_CLI" ] && V_CLI="N/A"

echo "  Cargo.toml: $V_CARGO | main.rs: $V_MAIN | cli.rs: $V_CLI"
if [ "$V_CARGO" = "$V_MAIN" ] && [ "$V_CARGO" = "$V_CLI" ]; then
  green "Versions consistent ($V_CARGO)"
else
  red "Version mismatch — fix before tagging"
fi

# ─── 3. Clippy ────────────────────────────────────────
hdr "3. Clippy (-D warnings)"
CLIPPY_OUT=$(cargo clippy --all-targets --quiet -- -D warnings 2>&1)
CLIPPY_ERR=$(echo "$CLIPPY_OUT" | grep "^error" | wc -l)
if [ "$CLIPPY_ERR" -gt 0 ]; then
  red "Clippy: $CLIPPY_ERR errors"
  echo "$CLIPPY_OUT" | grep "^error" | head -5
else
  green "Clippy clean (0 errors)"
fi

# ─── 4. Unit Tests ────────────────────────────────────
hdr "4. Unit Tests"
TEST_OUT=$(cargo test --quiet 2>&1)
PASSED=$(echo "$TEST_OUT" | grep -o '[0-9]* passed' | awk '{sum+=$1}END{print sum+0}')
FAILED=$(echo "$TEST_OUT" | grep -o '[0-9]* failed' | awk '{sum+=$1}END{print sum+0}')
echo "  passed=$PASSED  failed=$FAILED"
if [ "$FAILED" = "0" ]; then
  green "All $PASSED tests pass"
else
  red "$FAILED tests failed"
fi

# ─── 5. Release Binary ────────────────────────────────
hdr "5. Release Binary"
if cargo build --release --quiet; then
  if [ ! -f "$AGENT" ]; then
    red "Binary not found after release build"
  else
    BINVER=$($AGENT --version 2>&1 | head -1)
    echo "  $BINVER"
    if echo "$BINVER" | grep -q "$V_CARGO"; then
      green "Binary version matches ($V_CARGO)"
    else
      yellow "Binary version does not match Cargo.toml"
    fi
  fi
else
  red "cargo build --release failed"
fi

# ─── 6. Unicode Safety ────────────────────────────────
hdr "6. Unicode Safety (byte slicing scan)"
DANGER=$(grep -rn '\.\.[0-9]\+\]' src/ 2>/dev/null \
  | grep -v '//\|#\[allow\|chars()\|test\|bench\|\.toml\|\.txt' \
  | grep -v 'bytes\|ascii\|utf8_slice\|is_char_boundary' \
  | head -10)
if [ -z "$DANGER" ]; then
  green "No raw byte slicing detected in src/"
else
  yellow "Potential byte slicing (check manually):"
  echo "$DANGER"
fi

# ─── 7. Trajectories ──────────────────────────────────
hdr "7. Trajectories"
if [ -d fixtures/trajectories ]; then
  TOTAL=$(find fixtures/trajectories -name "*.json" | wc -l)
  EMPTY=$(find fixtures/trajectories -name "*.json" -empty | wc -l)
  echo "  Total JSON files: $TOTAL | Empty: $EMPTY"
  if [ "$EMPTY" = "0" ]; then
    green "All $TOTAL trajectory files non-empty"
  else
    yellow "$EMPTY empty trajectory files found"
  fi
  echo "  Directories:"
  ls fixtures/trajectories/ | head -10
else
  yellow "fixtures/trajectories/ not found"
fi

# ─── 8. Code Quality Indicators ───────────────────────
hdr "8. Code Quality Indicators"
DEAD=$(grep -REn '^#!?\[allow\(dead_code\)\]' src/ 2>/dev/null | grep -v test | wc -l)
echo "  #[allow(dead_code)] in src/: $DEAD"
if [ "$DEAD" -gt 10 ]; then
  yellow "Many dead_code allows ($DEAD) — consider cleanup"
else
  green "Dead code allows acceptable: $DEAD"
fi

if [ -f src/provider.rs ]; then
  LINES=$(wc -l < src/provider.rs)
  yellow "src/provider.rs still present ($LINES lines) — legacy code"
else
  green "Legacy provider.rs absent"
fi

ALLOW_UNUSED=$(grep -rn "#\[allow(unused" src/ 2>/dev/null | wc -l)
echo "  #[allow(unused*)] occurrences: $ALLOW_UNUSED"

# ─── 9. SWE-Bench Replay ──────────────────────────────
hdr "9. SWE-Bench Replay (offline — may take ~5 min)"
if [ ! -f "$AGENT" ]; then
  red "Binary not found — skipping bench-swe"
else
  echo "  Mode: $MODE_LABEL"
  SWE_CMD=("$AGENT" bench-swe --lang all --replay)
  if [ -n "$RERECORD_FLAG" ]; then
    SWE_CMD+=("$RERECORD_FLAG")
  fi

  SWE_OUT=$("${SWE_CMD[@]}" 2>&1)
  SWE_SUMMARY=$(echo "$SWE_OUT" | grep -E 'المجموع: *[0-9]+/[0-9]+|Passed: *[0-9]+/[0-9]+' | tail -1)
  echo "  ${SWE_SUMMARY:-<no summary line found>}"

  SWE_RATIO=$(echo "$SWE_SUMMARY" | grep -Eo '[0-9]+/[0-9]+' | head -1)
  SWE_PASS=$(echo "$SWE_RATIO" | cut -d/ -f1)
  SWE_TOTAL=$(echo "$SWE_RATIO" | cut -d/ -f2)

  if [ -z "$SWE_PASS" ] || [ -z "$SWE_TOTAL" ]; then
    yellow "Could not parse bench-swe result"
    echo "$SWE_OUT" | tail -10
  elif [ "$SWE_PASS" = "$SWE_TOTAL" ]; then
    green "bench-swe $SWE_PASS/$SWE_TOTAL ✅"
  elif [ "$SWE_PASS" -ge $((SWE_TOTAL - 2)) ]; then
    yellow "bench-swe $SWE_PASS/$SWE_TOTAL (acceptable, but check failures)"
  else
    red "bench-swe $SWE_PASS/$SWE_TOTAL — too low for release"
  fi
fi

# ─── 10. General Bench Replay ─────────────────────────
hdr "10. General Bench Replay (suite all)"
if [ ! -f "$AGENT" ]; then
  red "Binary not found — skipping bench"
else
  echo "  Mode: $MODE_LABEL"
  BENCH_CMD=("$AGENT" bench --suite all --replay)
  if [ -n "$RERECORD_FLAG" ]; then
    BENCH_CMD+=("$RERECORD_FLAG")
  fi

  BENCH_OUT=$("${BENCH_CMD[@]}" 2>&1)
  BENCH_LINES=$(echo "$BENCH_OUT" | grep -E "Passed:|Success Rate:" | head -3)
  echo "$BENCH_LINES"

  BENCH_RATE=$(echo "$BENCH_OUT" | grep "Success Rate:" | grep -Eo '[0-9.]*%' | head -1)
  if echo "$BENCH_RATE" | grep -q "^100"; then
    green "bench 100% ✅"
  elif [ -n "$BENCH_RATE" ]; then
    yellow "bench $BENCH_RATE — check failures"
  else
    yellow "Could not parse bench result"
  fi
fi

# ─── 11. Smoke Test ───────────────────────────────────
hdr "11. Smoke Test (replay)"
if [ ! -f sel_smoke_test.sh ]; then
  yellow "sel_smoke_test.sh not found"
elif [ ! -f "$AGENT" ]; then
  red "Binary not found — skipping smoke"
else
  SMOKE_OUT=$(bash sel_smoke_test.sh "$AGENT" --replay 2>&1)
  SMOKE_LINE=$(echo "$SMOKE_OUT" | grep -E "نجح:|Passed:|ALL PASSED" | tail -3)
  echo "$SMOKE_LINE"
  if echo "$SMOKE_OUT" | grep -q "ALL PASSED"; then
    green "Smoke 12/12 ✅"
  else
    SMOKE_PASS=$(echo "$SMOKE_OUT" | grep -o 'نجح: *[0-9]*' | grep -o '[0-9]*' | tail -1)
    if [ -n "$SMOKE_PASS" ] && [ "$SMOKE_PASS" = "12" ]; then
      green "Smoke 12/12 ✅"
    else
      red "Smoke test failures detected — check output"
    fi
  fi
fi

# ─── 12. API Keys (health) ────────────────────────────
hdr "12. API Keys & Providers"
if [ -f "$AGENT" ]; then
  $AGENT health 2>&1 | head -30
else
  yellow "Binary not found — skipping health check"
fi

# ─── 13. Git Status ───────────────────────────────────
hdr "13. Git Status"
echo "  Recent commits:"
git log --oneline -5 2>/dev/null || yellow "Not a git repo"
DIRTY=$(git status --porcelain 2>/dev/null | wc -l)
if [ "$DIRTY" = "0" ]; then
  green "Working tree clean"
else
  yellow "$DIRTY uncommitted changes:"
  git status --short | head -10
fi
CURRENT_TAG=$(git describe --tags --exact-match 2>/dev/null || echo "no tag")
echo "  Current tag: $CURRENT_TAG"

# ─── 14. Codebase Overview ────────────────────────────
hdr "14. Codebase Overview"
TOTAL_LINES=$(find src/ -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}')
TOTAL_FILES=$(find src/ -name "*.rs" 2>/dev/null | wc -l)
echo "  Rust files: $TOTAL_FILES | Total lines: $TOTAL_LINES"
echo "  Largest files:"
find src/ -name "*.rs" 2>/dev/null | xargs wc -l 2>/dev/null | sort -rn | head -8

# ─── Summary ──────────────────────────────────────────
echo ""
echo "╔═══════════════════════════════════╗"
echo "║   SEL Agent Health Check Summary  ║"
echo "╠═══════════════════════════════════╣"
printf "║  ✅ PASS: %-24s ║\n" "$PASS"
printf "║  ⚠️  WARN: %-24s ║\n" "$WARN"
printf "║  ❌ FAIL: %-24s ║\n" "$FAIL"
echo "╠═══════════════════════════════════╣"
if [ "$FAIL" = "0" ]; then
  echo "║  🎉 Ready for release             ║"
else
  echo "║  ⛔ Fix failures before release   ║"
fi
echo "╚═══════════════════════════════════╝"
