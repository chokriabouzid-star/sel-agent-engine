#!/bin/bash
# ══════════════════════════════════════════════════════════════
#   SEL Agent v1.3 — Full Feature Benchmark
#   يختبر كل مميزات v1.3 بشكل شامل
#   الهدف: 12/12
# ══════════════════════════════════════════════════════════════

AGENT=./target/release/sel-agent
WS=~/test-workspace
PASS=0
FAIL=0
RESULTS=()

run_test() {
    local id="$1"
    local label="$2"
    local goal="$3"
    local extra="${4:---max-repairs 3}"

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  TEST $id — $label"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    rm -rf "$WS"
    result=$($AGENT run --workspace "$WS" --goal "$goal" $extra 2>&1 | tail -1)

    if echo "$result" | grep -q "SEL_SUCCESS"; then
        echo "  ✅ TEST $id — $label"
        RESULTS+=("  ✅ TEST $id — $label")
        PASS=$((PASS + 1))
    else
        echo "  ❌ TEST $id — $label | $result"
        RESULTS+=("  ❌ TEST $id — $label | $result")
        FAIL=$((FAIL + 1))
    fi
    sleep 8
}

# ── L1: Basic Python ──────────────────────────────────────────

run_test "L1-T1" "Simple function" \
"Create add(a,b) in math_utils.py. Write pytest tests for positive, negative and zero values. Install pytest. Run tests."

run_test "L1-T2" "String utility" \
"Create reverse_string(s) in string_utils.py. Write pytest tests including empty string and unicode. Install pytest. Run tests."

run_test "L1-T3" "StdlibConflict — numbers.py" \
"Create is_even(n) in numbers.py but initially implement it incorrectly. Write pytest tests covering several numbers. Install pytest. Run tests."

run_test "L1-T4" "StdlibConflict — string.py" \
"Create capitalize_words(s) in string.py. Write pytest tests for single word, multiple words and empty string. Install pytest. Run tests."

# ── L2: Project Structure ────────────────────────────────────

run_test "L2-T5" "Subdirectory import" \
"Create project with calc.py containing add(a,b) and subtract(a,b). Write tests in tests/test_calc.py importing calc. Use pytest. Install pytest. Run tests."

run_test "L2-T6" "Multi-file package" \
"Create calculator package with operations.py containing add, subtract, multiply, divide. Create CLI calc.py using argparse. Write pytest tests for all operations. Install pytest. Run tests."

# ── L3: Self-Repair ──────────────────────────────────────────

run_test "L3-T7" "Logic bug repair" \
"Create is_palindrome(s) in palindrome.py but initially implement it incorrectly. Write pytest tests for several strings. Install pytest. Run tests."

run_test "L3-T8" "Import error repair" \
"Create DataProcessor class in processor.py that reads a CSV file and returns row count. Write pytest tests using a temporary CSV file. Install pytest. Run tests."

# ── L4: Mutation Testing ─────────────────────────────────────

run_test "L4-T9" "Mutation enforcement" \
"Create factorial(n) in math_utils.py. Write strong pytest tests including edge cases 0,1,5,10 and negative input handling. Install pytest. Run tests."

run_test "L4-T10" "Mutation — comparison operators" \
"Create grade(score) in grades.py that returns A for 90+, B for 80+, C for 70+, F otherwise. Write comprehensive pytest tests for boundary values. Install pytest. Run tests."

# ── L5: Web & Chaos ──────────────────────────────────────────

run_test "L5-T11" "Flask API" \
"Create Flask app with endpoints: GET /add?a=1&b=2 returns JSON sum, GET /multiply?a=2&b=3 returns JSON product. Write pytest tests using flask test client. Install flask pytest. Run tests."

run_test "L5-T12" "FastAPI + SQLite" \
"Create FastAPI app with SQLite database. Endpoints: POST /users with name field creates user, GET /users returns all users. Write pytest tests using TestClient. Install fastapi uvicorn pytest httpx. Run tests."

# ── RESULTS ──────────────────────────────────────────────────

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK v1.3 FULL RESULTS"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
for r in "${RESULTS[@]}"; do
    echo "$r"
done
echo ""
echo "  PASSED: $PASS / $((PASS + FAIL))"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
