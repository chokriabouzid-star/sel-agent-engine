#!/bin/bash
AGENT=./target/release/sel-agent
PASS=0
FAIL=0
TOTAL=0
REPAIRS=0

run_test() {
    local name=$1
    local workspace=$2
    local goal=$3
    sleep 15
    rm -rf "$workspace"
    TOTAL=$((TOTAL + 1))
    echo -n "   ⏳ Running: $name..."
    result=$($AGENT run --workspace "$workspace" --goal "$goal" --max-repairs 2 2>&1)
    repair_count=$(echo "$result" | grep -c "🔧 Repair")
    REPAIRS=$((REPAIRS + repair_count))
    if echo "$result" | grep -q "SEL_SUCCESS"; then
        echo "✅ $name (repairs: $repair_count)"
        PASS=$((PASS + 1))
    else
        echo "❌ $name (repairs: $repair_count)"
        echo "   └─ $(echo "$result" | grep "Failure type\|Last errors" | head -2)"
        FAIL=$((FAIL + 1))
    fi
}

bash health_check.sh
echo ""
echo "=== SEL Stress Benchmark ==="
echo ""

echo "── Python ──"
run_test "broken import"     ~/bm/py_f1 "Create service.py that does import pandas but pandas is not installed. Write test_service.py that imports service. Install pytest but NOT pandas. Run tests."
run_test "wrong assertion"   ~/bm/py_f2 "Create math_ops.py with add(a,b) returning a*b (wrong). test_math_ops.py expects a+b. Install pytest. Run tests."
run_test "wrong signature"   ~/bm/py_f3 "Create utils.py with greet(name) but test calls greet() with no args. Install pytest. Run tests."

echo ""
echo "── Rust ──"
run_test "wrong logic"       ~/bm/rs_f1 "Create Rust lib. Cargo.toml and src/lib.rs. add(a,b) returns a-b. Tests expect a+b. Run tests."
run_test "type mismatch"     ~/bm/rs_f2 "Create Rust lib. Cargo.toml and src/lib.rs. Function add(a: i32, b: i32) -> String returns a+b as integer (type error). Tests expect String. Run tests."
run_test "missing closing"   ~/bm/rs_f3 "Create Rust lib. Cargo.toml and src/lib.rs. Write add function but forget closing brace. Tests inside lib.rs. Run tests."

echo ""
echo "── Go ──"
run_test "undefined func"    ~/bm/go_f1 "Create Go module gotest. math.go with Add only. test calls Multiply which doesnt exist. Run tests."
run_test "wrong logic"       ~/bm/go_f2 "Create Go module gotest. Add(a,b int) returns a-b. TestAdd expects a+b. Run tests."
run_test "syntax error"      ~/bm/go_f3 "Create Go module gotest. math.go with Add but missing closing brace. TestAdd in math_test.go. Run tests."

echo ""
echo "── Node.js ──"
run_test "wrong return"      ~/bm/nd_f1 "Create calc.js with add(a,b) returning a*b (wrong). test_calc.js asserts add(2,3)===5. Run tests."
run_test "missing function"  ~/bm/nd_f2 "Create utils.js with reverse only. test_utils.js calls uppercase which doesnt exist. Run tests."
run_test "runtime error"     ~/bm/nd_f3 "Create parser.js with parse(text) that calls JSON.parse but test passes undefined. Run tests."

echo ""
avg_repairs=$(echo "scale=1; $REPAIRS / $TOTAL" | bc)
echo "=== Stress Results: $PASS/$TOTAL passed | avg repairs: $avg_repairs ==="
rm -rf ~/bm
