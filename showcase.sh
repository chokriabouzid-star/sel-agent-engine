#!/bin/bash
AGENT=./target/release/sel-agent
PASS=0; FAIL=0

run_test() {
    local name=$1 workspace=$2 goal=$3
    sleep 25
    rm -rf "$workspace"
    result=$($AGENT run --workspace "$workspace" --goal "$goal" --max-repairs 3 2>&1)
    if echo "$result" | grep -qE "All tests passed|Goal complete"; then
        echo "✅ $name"
        PASS=$((PASS + 1))
    else
        echo "❌ $name"
        FAIL=$((FAIL + 1))
    fi
}

echo "╔══════════════════════════════════════════╗"
echo "║     SEL Agent v1.0 — Real World Tests    ║"
echo "╚══════════════════════════════════════════╝"
echo ""

echo "── Test 1: REST API (Python) ──"
run_test "FastAPI CRUD + SQLite" ~/showcase/t1 \
"Create FastAPI app: POST /users (name,email), GET /users, GET /users/{id}, DELETE /users/{id}. SQLite with :memory: in tests. test_main.py with TestClient. Install fastapi pytest httpx uvicorn. Run tests."

echo ""
echo "── Test 2: Data Processing (Python) ──"
run_test "CSV Analyzer + Stats" ~/showcase/t2 \
"Create analyzer.py: read CSV with columns name,score,grade. Functions: average_score(), top_students(n), grade_distribution(). Generate sample CSV in tests. Install pytest. Run tests."

echo ""
echo "── Test 3: Systems (Rust) ──"
run_test "Rust: File Word Counter" ~/showcase/t3 \
"Create Rust CLI. Cargo.toml and src/lib.rs directly. Functions: count_words(text) -> usize, count_lines(text) -> usize, most_common_word(text) -> String. Tests in lib.rs. Run tests."

echo ""
echo "── Test 4: Concurrent (Go) ──"
run_test "Go: Worker Pool" ~/showcase/t4 \
"Create Go module workers. worker.go with ProcessJobs(jobs []int, workers int) []int that squares each number using goroutines. worker_test.go with TestProcessJobs testing correctness and order. Run tests."

echo ""
echo "── Test 5: Multi-Agent Script (Python) ──"
run_test "LLM Debate Script" ~/showcase/t5 \
"Create debate.py: takes topic argument, calls Groq API (GROQ_API_KEY, llama-3.3-70b-versatile) — Agent A argues FOR, Agent B argues AGAINST, Agent A gives consensus. test_debate.py mocks httpx calls. Install httpx pytest pytest-mock. Run tests."

echo ""
echo "══════════════════════════════════════════"
echo "Results: $PASS/5 passed"
echo "══════════════════════════════════════════"
