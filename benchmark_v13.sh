#!/bin/bash
# benchmark_v13.sh — SEL Agent v1.3 Full Test Suite

AGENT=~/projects/sel-agent-v4/target/release/sel-agent
WS=~/test-workspace
PASS=0
FAIL=0
RESULTS=()

run_test() {
    local id="$1"
    local label="$2"
    local goal="$3"

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  TEST $id — $label"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    rm -rf "$WS"
    OUTPUT=$($AGENT run --workspace "$WS" --goal "$goal" --max-repairs 3 2>&1)
    echo "$OUTPUT"

    if echo "$OUTPUT" | grep -q "SEL_SUCCESS"; then
        PASS=$((PASS + 1))
        RESULTS+=("✅ TEST $id — $label")
    else
        FAIL=$((FAIL + 1))
        REASON=$(echo "$OUTPUT" | grep "SEL_FAILED" | head -1)
        RESULTS+=("❌ TEST $id — $label | $REASON")
    fi

    sleep 10
}

# ── Level 1: Happy Path ───────────────────────────────────────────────────────
run_test "L1-T1" "Simple multiply" \
  "Create multiply(a,b) in math_utils.py. Write pytest tests for positive, negative and zero values. Install pytest. Run tests."

run_test "L1-T2" "String utility" \
  "Create reverse_string(s) in string_utils.py. Write pytest tests including empty string and unicode text. Install pytest. Run tests."

# ── Level 2: Import Errors ────────────────────────────────────────────────────
run_test "L2-T3" "Subdirectory import" \
  "Create project with calc.py containing add(a,b). Write tests in tests/test_calc.py importing calc. Use pytest. Install pytest. Run tests."

# ── Level 3: Logic Bugs ───────────────────────────────────────────────────────
run_test "L3-T4" "Logic repair" \
  "Create is_even(n) in numbers_utils.py but initially implement it incorrectly. Write pytest tests covering several numbers. Install pytest. Run tests."

# ── Level 4: Multi-File ───────────────────────────────────────────────────────
run_test "L4-T5" "Calculator package" \
  "Create calculator package with add, subtract, multiply, divide in operations.py. Create CLI calc.py using argparse. Write pytest tests for operations. Install pytest. Run tests."

# ── Level 5: Hard ─────────────────────────────────────────────────────────────
run_test "L5-T6" "JSON parser" \
  "Implement simple JSON parser supporting numbers, strings and objects in json_parser.py. Write pytest tests validating parsing behavior. Install pytest. Run tests."

# ── Mutation Strength ─────────────────────────────────────────────────────────
run_test "MUT-T7" "Mutation enforcement" \
  "Create factorial(n) in math_utils.py. Write strong pytest tests including edge cases 0,1,5,10 and negative input handling. Install pytest. Run tests."

# ── Chaos ─────────────────────────────────────────────────────────────────────
run_test "CHX-T8" "Flask chaos" \
  "Create small Flask app with endpoint /add returning JSON sum of a and b. Write pytest tests using flask test client. Install dependencies. Run tests."

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK v1.3 RESULTS"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
for r in "${RESULTS[@]}"; do echo "  $r"; done
echo ""
echo "  PASSED: $PASS / $((PASS + FAIL))"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
