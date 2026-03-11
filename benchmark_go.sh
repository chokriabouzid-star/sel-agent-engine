#!/bin/bash
AGENT=~/projects/sel-agent-v4/target/release/sel-agent
PASS=0; FAIL=0; RESULTS=()

sleep 60 && echo "⏳ Warm-up delay done..."

run_test() {
    local id="$1" name="$2" workspace="$3" goal="$4"
    echo ""; echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "▶ $id — $name"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    rm -rf "$workspace"
    result=$($AGENT run --workspace "$workspace" --goal "$goal" --max-repairs 5 2>&1)
    status=$(echo "$result" | grep -o "SEL_SUCCESS\|SEL_FAILED" | tail -1)
    repairs=$(echo "$result" | grep -c "Repair [0-9]*/5")
    if [ "$status" = "SEL_SUCCESS" ]; then
        echo "✅ $id PASSED (repairs: $repairs)"
        PASS=$((PASS + 1)); RESULTS+=("✅ $id — $name (repairs: $repairs)")
    else
        echo "❌ $id FAILED"
        echo "$result" | grep -E "error\[|^error" | head -5
        FAIL=$((FAIL + 1)); RESULTS+=("❌ $id — $name")
    fi
    sleep 60
}

run_test "G1" "net/http + os.ReadFile (no ioutil)" ~/go-test-g1 \
"Build a Go HTTP server. go.mod module 'g1' go 1.21. Routes: GET /health returns {status:ok}, GET /read reads local file 'data.txt' using os.ReadFile (NOT ioutil). Write Go tests using net/http/httptest. Run go test ./..."

run_test "G2" "chi router + middleware" ~/go-test-g2 \
"Build a Go REST API using chi router. go.mod module 'g2' go 1.21. Use github.com/go-chi/chi/v5. Routes: GET /users returns JSON list, POST /users creates user. Add chi middleware: Logger and Recoverer. Write Go tests using httptest. Run go test ./..."

run_test "G3" "JWT with golang-jwt/jwt v5" ~/go-test-g3 \
"Build JWT authentication in Go. go.mod module 'g3' go 1.21. Use github.com/golang-jwt/jwt/v5. Function GenerateToken(userID string, secret string) returns signed token. Function ValidateToken(token string, secret string) returns userID or error. Write Go tests: generate then validate, test invalid token returns error. Run go test ./..."

run_test "G4" "GORM v2 + SQLite" ~/go-test-g4 \
"Build Go app with GORM v2 and SQLite. go.mod module 'g4' go 1.21. Use gorm.io/gorm and gorm.io/driver/sqlite (NOT github.com/jinzhu/gorm). User struct with ID Name Email. Implement CreateUser GetUser ListUsers DeleteUser. Use ':memory:' SQLite. Write Go tests for all CRUD. Run go test ./..."

run_test "G5" "Goroutines + channels + sync.WaitGroup" ~/go-test-g5 \
"Build concurrent worker pool in Go. go.mod module 'g5' go 1.21. WorkerPool struct with configurable workers (default 3). Submit(job func()) adds to channel. Workers process concurrently with goroutines. WaitAll() blocks until done using sync.WaitGroup. No data races. Write Go tests: submit 10 jobs verify all executed. Run go test -race ./..."

echo ""
echo "╔══════════════════════════════════════════╗"
echo "║      Go Benchmark Results                ║"
echo "╚══════════════════════════════════════════╝"
for r in "${RESULTS[@]}"; do echo "  $r"; done
echo ""; echo "  PASSED: $PASS / $((PASS + FAIL))"; echo ""
