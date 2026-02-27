#!/bin/bash
AGENT=./target/release/sel-agent
PASS=0
FAIL=0
TOTAL=0

run_test() {
    local name=$1
    local workspace=$2
    local goal=$3
    rm -rf "$workspace"
    TOTAL=$((TOTAL + 1))
    result=$($AGENT run --workspace "$workspace" --goal "$goal" --max-repairs 2 2>&1)
    if echo "$result" | grep -q "All tests passed\|Goal complete"; then
        echo "✅ $name"
        PASS=$((PASS + 1))
    else
        echo "❌ $name"
        FAIL=$((FAIL + 1))
    fi
}

echo "=== SEL Benchmark ==="

# Python
run_test "Python: stats"        ~/bm/py1 "Create stats.py with calculate(numbers) returns min/max/mean dict. Use stdlib. Write test_stats.py. Install pytest. Run tests."
run_test "Python: SQLite"       ~/bm/py2 "Create db.py with SQLite init_db/add_user/get_user/list_users. pytest with autouse fixture. Install pytest. Run tests."
run_test "Python: FastAPI"      ~/bm/py3 "Create FastAPI app main.py GET /items POST /items with Pydantic. test_main.py with TestClient. Install fastapi pytest httpx uvicorn. Run tests."

# Rust  
run_test "Rust: math lib"       ~/bm/rs1 "Create Rust lib. Cargo.toml and src/lib.rs directly. Functions add/subtract/multiply. Tests inside lib.rs. Run tests."
run_test "Rust: string ops"     ~/bm/rs2 "Create Rust lib. Cargo.toml and src/lib.rs. Function reverse_string(s: &str) -> String. Tests inside lib.rs. Run tests."

# Go
run_test "Go: math"             ~/bm/go1 "Create Go module gomath. math.go with Add/Subtract. math_test.go with TestAdd TestSubtract. Run tests."
run_test "Go: strings"          ~/bm/go2 "Create Go module gostr. strings.go with Reverse(s string) string. strings_test.go with TestReverse. Run tests."

# Node.js
run_test "Node: calculator"     ~/bm/nd1 "Create calculator.js with add/subtract/multiply functions. test_calc.js with assert tests. Run tests."
run_test "Node: string utils"   ~/bm/nd2 "Create stringUtils.js with reverse/uppercase functions. test_strings.js using assert. Run tests."

echo ""
echo "=== Results: $PASS/$TOTAL passed ==="
rm -rf ~/bm
