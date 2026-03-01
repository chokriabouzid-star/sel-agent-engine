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
echo "=== Half 2: Go + Node ==="
run_test "Go: undefined func"  ~/bm/go_f1 "Create Go module gotest. math.go with Add only. test calls Multiply which doesnt exist. Run tests."
run_test "Go: wrong logic"     ~/bm/go_f2 "Create Go module gotest. Add(a,b int) returns a-b. TestAdd expects a+b. Run tests."
run_test "Go: syntax error"    ~/bm/go_f3 "Create Go module gotest. math.go with Add but missing closing brace. TestAdd in math_test.go. Run tests."
run_test "Node: wrong return"  ~/bm/nd_f1 "Create calc.js with add(a,b) returning a*b (wrong). test_calc.js asserts add(2,3)===5. Run tests."
run_test "Node: missing func"  ~/bm/nd_f2 "Create utils.js with reverse only. test_utils.js calls uppercase which doesnt exist. Run tests."
run_test "Node: runtime error" ~/bm/nd_f3 "Create parser.js with parse(text) that calls JSON.parse but test passes undefined. Run tests."

avg=$(echo "scale=1; $REPAIRS / $TOTAL" | bc)
echo "=== Results: $PASS/$TOTAL | avg repairs: $avg ==="
rm -rf ~/bm
