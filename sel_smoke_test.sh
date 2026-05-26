#!/usr/bin/env bash
# ============================================================
# SEL Agent — Smoke Test Suite (Release Validation)
# تحقق من الاستقرار قبل الإطلاق على GitHub
# الاستخدام: bash sel_smoke_test.sh [sel-agent-path]
# ============================================================

set -euo pipefail
export SEL_BENCH_MODE=1

SEL="${1:-./target/debug/sel-agent}"
EXTRA_ARGS="${@:2}"
PASS=0
FAIL=0
SKIP=0
RESULTS=()
START_TIME=$(date +%s)

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

# ── helpers ──────────────────────────────────────────────────
run_case() {
    local id="$1"
    local lang="$2"
    local workspace
    workspace=$(mktemp -d /tmp/sel-smoke-XXXXXX)
    local goal="$3"

    echo -e "\n${BLUE}▶ [$id]${NC} $lang"
    echo -e "  goal: ${YELLOW}${goal:0:80}...${NC}"

    local output
    local exit_code=0

    output=$("$SEL" run \
        --workspace "$workspace" \
        --goal "$goal" \
        --max-repairs 3 \
        $EXTRA_ARGS \
        2>&1) || exit_code=$?

    rm -rf "$workspace"

    if echo "$output" | grep -q "SEL_SUCCESS"; then
        echo -e "  ${GREEN}✅ PASSED${NC}"
        PASS=$((PASS + 1))
        RESULTS+=("PASS|$id|$lang")
    else
        local last_err
        last_err=$(echo "$output" | grep -E "error|Error|FAILED|❌" | tail -3)
        echo -e "  ${RED}❌ FAILED${NC}"
        echo -e "  ${RED}$last_err${NC}"
        FAIL=$((FAIL + 1))
        RESULTS+=("FAIL|$id|$lang|$last_err")
    fi
}

banner() {
    echo -e "\n${BOLD}╔══════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║   SEL Agent — Smoke Test Suite           ║${NC}"
    echo -e "${BOLD}║   Release Validation v1.0                ║${NC}"
    echo -e "${BOLD}╚══════════════════════════════════════════╝${NC}\n"
}

# ── تحقق من وجود الـ binary ──────────────────────────────────
check_binary() {
    if ! command -v "$SEL" &>/dev/null && ! test -f "$SEL"; then
        echo -e "${RED}Error: sel-agent not found at: $SEL${NC}"
        echo "Build first: cargo build"
        exit 1
    fi
    echo -e "${GREEN}✓ sel-agent found: $SEL${NC}"
}

# ============================================================
# المهام الثماني — مختلفة تماماً عن bench cases
# ============================================================

banner
check_binary
echo -e "${BOLD}Running 8 real-world tasks...${NC}"

# ── 1. Python: إصلاح خطأ في خوارزمية البحث الثنائي ──────────
run_case "smoke_py_binary_search" "python" \
"Fix this broken binary search implementation. The function should return \
the index of target in a sorted list, or -1 if not found. \
Write binary_search.py with the fixed implementation and \
test_binary_search.py that tests: found in middle, found at start, \
found at end, not found, empty list."

# ── 2. Python: كلاس مع properties ──────────────────────────────
run_case "smoke_py_temperature" "python" \
"Create a Temperature class in Python that stores temperature in Celsius. \
It should have: a constructor accepting celsius value, a to_fahrenheit() method \
(formula: F = C * 9/5 + 32), a to_kelvin() method (formula: K = C + 273.15), \
and a __str__ method. Write temperature.py and test_temperature.py with \
at least 4 tests covering conversions and edge cases like 0C and -273.15C."

# ── 3. Go: concurrent worker pool ───────────────────────────────
run_case "smoke_go_worker_pool" "go" \
"Create a simple Go worker pool. Write a function ProcessJobs(jobs []int, workers int) []int \
that processes each job by squaring it using goroutines. \
Use go mod init workertest. Write main.go with the implementation and \
main_test.go that tests: 5 workers processing 10 jobs, \
single worker, more workers than jobs."

# ── 4. Go: مدير stack بـ generics ────────────────────────────────
run_case "smoke_go_generics" "go" \
"Create a generic stack in Go using type parameters (Go 1.18+). \
The Stack[T any] type should have: Push(val T), Pop() (T, bool), \
Peek() (T, bool), IsEmpty() bool, Size() int methods. \
Use go mod init genericstack. Write main.go and main_test.go that \
tests integer stack, string stack, and empty stack behavior."

# ── 5. TypeScript: utility functions مع types ────────────────────
run_case "smoke_ts_utils" "typescript" \
"Create TypeScript utility functions in utils.ts: \
1) chunk<T>(arr: T[], size: number): T[][] — splits array into chunks \
2) flatten<T>(arr: T[][]): T[] — flattens 2D array \
3) unique<T>(arr: T[]): T[] — removes duplicates \
Export all functions. Write utils.test.ts with Jest tests for each function \
including edge cases like empty arrays and size=1 chunks."

# ── 6. TypeScript: async retry function ──────────────────────────
run_case "smoke_ts_retry" "typescript" \
"Create a TypeScript function retry<T>(fn: () => Promise<T>, attempts: number, delayMs: number): Promise<T> \
that retries a failing async function up to N times with delay between attempts. \
Write retry.ts with the implementation and retry.test.ts that tests: \
succeeds on first try, succeeds after 2 failures, fails after max attempts. \
Use jest.useFakeTimers() to avoid real delays in tests. IMPORTANT Jest rule: When using fake timers, always use await jest.advanceTimersByTimeAsync(ms) instead of advanceTimersByTime to ensure microtasks resolve correctly."

# ── 7. Rust: CSV parser بسيط ─────────────────────────────────────
run_case "smoke_rust_csv" "rust" \
"Create a simple CSV line parser in Rust using cargo new csvparser --lib. \
Write a function parse_csv_line(line: &str) -> Vec<String> that splits a \
CSV line by comma and trims whitespace from each field. \
Write implementation and #[cfg(test)] unit tests in src/lib.rs. \
Test cases: normal CSV line, line with spaces, empty fields, single field."

# ── 8. Rust: خوارزمية fibonacci مع cache ─────────────────────────
run_case "smoke_rust_fib" "rust" \
"Create a Fibonacci calculator in Rust using cargo new fibcache --lib. \
Write two functions in src/lib.rs: \
fib_recursive(n: u64) -> u64 (simple recursive), \
fib_iterative(n: u64) -> u64 (iterative, efficient). \
Include #[cfg(test)] tests that verify both functions give same results for \
n=0,1,5,10,20 and that fib(0)=0, fib(1)=1, fib(10)=55."

# ── 9. TypeScript: FastAPI Client ───────────────────────────────
run_case "smoke_ts_fastapi_client" "typescript" \
"Write a TypeScript client in api.ts for a FastAPI endpoint. \
It should have a class ApiClient with a method getUser(id: number): Promise<{id: number, name: string}>. \
Use axios. Write api.test.ts using jest.mock to test successful response and 404 error."

# ── 10. Go: Concurrent Mutex ────────────────────────────────────
run_case "smoke_go_concurrent" "go" \
"Write a Go struct SafeCounter with methods Inc(key string) and Value(key string) int. \
Use sync.Mutex to make it safe for concurrent use. \
Use go mod init counter. Write main.go and main_test.go that tests 1000 concurrent increments."

# ── 11. Python: Dataclass Validation ────────────────────────────
run_case "smoke_py_dataclass" "python" \
"Write a Python User dataclass in user.py with fields: username (str), age (int). \
Add a __post_init__ method that raises ValueError if age < 0 or username is empty. \
Write test_user.py with tests for valid creation, negative age, and empty username."

# ── 12. Rust: Trait Impl ────────────────────────────────────────
run_case "smoke_rust_trait_impl" "rust" \
"Implement the std::fmt::Display trait for a custom struct Point { x: i32, y: i32 } in Rust. \
Use cargo new pointfmt --lib. Format should be (x, y). \
Write #[cfg(test)] tests verifying formatting works correctly for positive and negative coordinates."

# ============================================================
# النتائج النهائية
# ============================================================
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))
TOTAL=$((PASS + FAIL))

echo -e "\n${BOLD}╔══════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║   Smoke Test Results                     ║${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════╣${NC}"
printf "${BOLD}║  %-38s ║${NC}\n" "Total:    $TOTAL / 12"
printf "${BOLD}║  %-38s ║${NC}\n" "Passed:   $PASS"
printf "${BOLD}║  %-38s ║${NC}\n" "Failed:   $FAIL"
printf "${BOLD}║  %-38s ║${NC}\n" "Duration: ${DURATION}s"
echo -e "${BOLD}╚══════════════════════════════════════════╝${NC}"

echo -e "\n${BOLD}── Details ──────────────────────────────────────${NC}"
for r in "${RESULTS[@]}"; do
    IFS='|' read -r status id lang rest <<< "$r"
    if [[ "$status" == "PASS" ]]; then
        echo -e "  ${GREEN}✅${NC} $id ($lang)"
    else
        echo -e "  ${RED}❌${NC} $id ($lang) — $rest"
    fi
done

echo ""
if [[ $FAIL -eq 0 ]]; then
    echo -e "${GREEN}${BOLD}🎉 ALL PASSED — Agent is stable. Ready for release.${NC}"
    exit 0
elif [[ $FAIL -le 1 ]]; then
    echo -e "${YELLOW}${BOLD}⚠  $FAIL/12 failed — Minor issues. Review before release.${NC}"
    exit 1
else
    echo -e "${RED}${BOLD}🚫 $FAIL/12 failed — Not ready for release.${NC}"
    exit 2
fi
