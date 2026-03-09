#!/bin/bash
AGENT=./target/release/sel-agent
WS=~/test-workspace
PASS=0; FAIL=0; RESULTS=()

run_test() {
    local id="$1" label="$2" goal="$3" repairs="${4:-5}"
    echo ""; echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  TEST $id — $label"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    rm -rf "$WS"
    result=$($AGENT run --workspace "$WS" --goal "$goal" --max-repairs "$repairs" 2>&1 | tail -1)
    if echo "$result" | grep -q "SEL_SUCCESS"; then
        echo "  ✅ TEST $id — $label"; RESULTS+=("  ✅ TEST $id — $label"); PASS=$((PASS+1))
    else
        # Retry once after 30s
        echo "  ⚠ First attempt failed — retrying in 30s..."
        sleep 30; rm -rf "$WS"
        result=$($AGENT run --workspace "$WS" --goal "$goal" --max-repairs "$repairs" 2>&1 | tail -1)
        if echo "$result" | grep -q "SEL_SUCCESS"; then
            echo "  ✅ TEST $id — $label (retry)"; RESULTS+=("  ✅ TEST $id — $label (retry)"); PASS=$((PASS+1))
        else
            echo "  ❌ TEST $id — $label"; RESULTS+=("  ❌ TEST $id — $label"); FAIL=$((FAIL+1))
        fi
    fi
    sleep 25
}

run_test "L6-T1" "Async Python + pytest-asyncio" \
"Create async function fetch_data(url: str) in fetcher.py that uses aiohttp to GET a URL and return status code. Mock the HTTP call in tests. Install pytest pytest-asyncio aiohttp. Write async tests. Run tests."

run_test "L6-T2" "CLI tool with Click" \
"Create CLI tool in cli.py using Click with two commands: greet NAME prints Hello NAME, add A B prints sum. Write pytest tests using Click CliRunner. Install click pytest. Run tests."

run_test "L6-T3" "Pandas data processing" \
"Create data_processor.py with clean_data(df) that removes null rows and duplicates, and normalize_scores(df, col) that normalizes column to 0-1. Write pytest tests using pandas DataFrames. Install pandas pytest. Run tests."

run_test "L6-T4" "OOP inheritance + mutation" \
"Create shapes.py with base class Shape with area() method, subclasses Circle(radius) and Rectangle(width, height). Initially implement area() incorrectly for one shape. Write comprehensive pytest tests. Install pytest. Run tests."

run_test "L6-T5" "Multi-module without circular imports" \
"Create project: models.py with User dataclass (id, name, email), validators.py with validate_email and validate_name, service.py with create_user using both. Write pytest tests. Install pytest. Run tests."

run_test "L6-T6" "FastAPI + JWT authentication" \
"Create FastAPI app with JWT auth. Endpoints: POST /register with username and password, POST /login returns JWT token, GET /profile requires Bearer token. Write pytest tests for all endpoints. Install fastapi uvicorn pytest httpx sqlalchemy python-jose passlib python-multipart. Run tests." 7

run_test "L6-T7" "Thread-safe counter" \
"Create thread_safe_counter.py with ThreadSafeCounter class using threading.Lock with increment(), decrement(), get_value(). Write pytest tests running 100 threads simultaneously. Install pytest. Run tests."

run_test "L6-T8" "Multiple stdlib conflicts" \
"Create project with: math.py containing distance(x1,y1,x2,y2), random.py containing shuffle_list(lst) and pick_random(lst), json.py containing parse_config(text). Write pytest tests for all. Install pytest. Run tests."

echo ""; echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK Level 6 RESULTS"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
for r in "${RESULTS[@]}"; do echo "$r"; done
echo ""; echo "  PASSED: $PASS / $((PASS+FAIL))"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
