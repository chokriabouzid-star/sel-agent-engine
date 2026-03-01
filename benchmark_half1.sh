#!/bin/bash
AGENT=./target/release/sel-agent
PASS=0; FAIL=0; TOTAL=0; REPAIRS=0

run_test() {
    local name=$1 workspace=$2 goal=$3
    sleep 10
    rm -rf "$workspace"
    TOTAL=$((TOTAL + 1))
    result=$($AGENT run --workspace "$workspace" --goal "$goal" --max-repairs 2 2>&1)
    repair_count=$(echo "$result" | grep -c "🔧 Repair")
    REPAIRS=$((REPAIRS + repair_count))
    if echo "$result" | grep -qE "All tests passed|Goal complete"; then
        echo "✅ $name (repairs: $repair_count)"; PASS=$((PASS + 1))
    else
        echo "❌ $name (repairs: $repair_count)"
        echo "   └─ $(echo "$result" | grep "Failure type" | head -2)"
        FAIL=$((FAIL + 1))
    fi
}

bash health_check.sh
echo ""
echo "=== Half 1: Python + Rust ==="
run_test "Python: broken import"  ~/bm/py_f1 "Create calculator.py with add(a,b) function. At the top accidentally import nonexistent_lib. Write test_calculator.py testing add(2,3)==5. Install pytest. Run tests."
run_test "Python: wrong assertion" ~/bm/py_f2 "Create math_ops.py with add(a,b) returning a*b (wrong). test_math_ops.py expects a+b. Install pytest. Run tests."
run_test "Python: wrong signature" ~/bm/py_f3 "Create utils.py with greet(name) but test calls greet() with no args. Install pytest. Run tests."
run_test "Rust: wrong logic"      ~/bm/rs_f1 "Create Rust lib. Cargo.toml and src/lib.rs directly. add(a,b) returns a-b. Tests expect a+b. Run tests."
run_test "Rust: type mismatch"    ~/bm/rs_f2 "Create Rust lib. Cargo.toml and src/lib.rs directly. Function add(a: i32, b: i32) -> String returns integer. Tests expect String. Run tests."
run_test "Rust: missing closing"  ~/bm/rs_f3 "Create Rust lib. Cargo.toml and src/lib.rs directly. Write add function but forget closing brace. Tests inside lib.rs. Run tests."

avg=$(echo "scale=1; $REPAIRS / $TOTAL" | bc)
echo "=== Results: $PASS/$TOTAL | avg repairs: $avg ==="
rm -rf ~/bm
