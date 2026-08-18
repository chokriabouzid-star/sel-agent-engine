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
    local case_num=$((PASS + FAIL + 1))

    echo -e "\n${BOLD}╔══════════════════════════════════════════╗${NC}"
    printf "${BOLD}║  [%02d/12] %-32s║${NC}\n" "$case_num" " $id"
    echo -e "${BOLD}╚══════════════════════════════════════════╝${NC}"
    echo -e "  ${BLUE}lang:${NC} $lang"
    echo -e "  ${YELLOW}goal:${NC} ${goal:0:100}..."
    echo -e ""

    local tmplog
    tmplog=$(mktemp /tmp/sel-smoke-log-XXXXXX)
    local case_start
    case_start=$(date +%s)

    # تشغيل الوكيل مع إظهار اللوج مباشرة (كما تفعل البانشات الأخرى)
    # نستخدم || true لمنع set -e من إيقاف السكربت عند فشل المهمة
    "$SEL" run \
        --workspace "$workspace" \
        --goal "$goal" \
        --max-repairs 3 \
        $EXTRA_ARGS \
        2>&1 | tee "$tmplog" || true

    local case_end elapsed
    case_end=$(date +%s)
    elapsed=$((case_end - case_start))

    local output
    output=$(cat "$tmplog")
    rm -f "$tmplog"
    rm -rf "$workspace" || true

    # استخراج عدد الإصلاحات من اللوج — || echo "0" يمنع set -e عند عدم وجود تطابق
    local repairs
    repairs=$(echo "$output" | grep -oP 'repair_count:\K[0-9]+' | tail -1 || echo "")
    if [[ -z "$repairs" ]]; then
        repairs=$(echo "$output" | grep -oP '\b([0-9]+) repair' | grep -oP '[0-9]+' | tail -1 || echo "0")
    fi
    repairs="${repairs:-0}"

    echo -e ""
    if echo "$output" | grep -q "SEL_SUCCESS" 2>/dev/null; then
        echo -e "${GREEN}${BOLD}  ✅ PASSED${NC}  ${BLUE}(${elapsed}s, repairs: ${repairs})${NC}"
        PASS=$((PASS + 1))
        RESULTS+=("PASS|$id|$lang|${elapsed}s|repairs:${repairs}")
    else
        local last_err
        last_err=$(echo "$output" | grep -E "error|Error|FAILED|❌|failed" 2>/dev/null | grep -v "^$" | tail -3 || echo "")
        echo -e "${RED}${BOLD}  ❌ FAILED${NC}  ${BLUE}(${elapsed}s, repairs: ${repairs})${NC}"
        if [[ -n "$last_err" ]]; then
            echo -e "  ${RED}↳ $last_err${NC}"
        fi
        FAIL=$((FAIL + 1))
        RESULTS+=("FAIL|$id|$lang|${elapsed}s|repairs:${repairs}")
    fi
    echo -e "${BOLD}──────────────────────────────────────────${NC}"
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
echo -e "${BOLD}Running 12 real-world tasks...${NC}"

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
single worker, more workers than jobs. \
The test for ProcessJobs must sort both the result and expected slices before comparing, since goroutine results are unordered. Use sort.Slice or sort.Ints on the result before asserting equality."

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
"Create a TypeScript async retry utility. Write ALL files in a SINGLE write_file call each — do NOT split one file across multiple write_file steps. \
File 1 — retry.ts (write completely in one shot): \
export async function retry<T>(fn: () => Promise<T>, attempts: number, delayMs: number): Promise<T> { \
  let lastError: unknown; \
  for (let i = 0; i < attempts; i++) { \
    try { return await fn(); } \
    catch (e) { \
      lastError = e; \
      if (i < attempts - 1) \
        await new Promise(r => setTimeout(r, delayMs)); \
    } \
  } \
  throw lastError; \
} \
File 2 — retry.test.ts (write exactly this code): \
import { retry } from './retry'; \
beforeEach(() => { jest.useFakeTimers(); }); \
afterEach(() => { jest.useRealTimers(); }); \
it('succeeds on first try', async () => { \
  const fn = jest.fn().mockResolvedValue('ok'); \
  await expect(retry(fn, 3, 100)).resolves.toBe('ok'); \
  expect(fn).toHaveBeenCalledTimes(1); \
}); \
it('fails twice then succeeds', async () => { \
  let calls = 0; \
  const fn = jest.fn(() => { \
    calls++; \
    if (calls < 3) return Promise.reject(new Error('fail')); \
    return Promise.resolve('ok'); \
  }); \
  const p = retry(fn, 3, 100); \
  await jest.runAllTimersAsync(); \
  await expect(p).resolves.toBe('ok'); \
  expect(fn).toHaveBeenCalledTimes(3); \
}); \
it('always fails', async () => { \
  const fn = jest.fn().mockRejectedValue(new Error('always')); \
  const p = retry(fn, 3, 100); \
  await jest.runAllTimersAsync(); \
  await expect(p).rejects.toThrow('always'); \
  expect(fn).toHaveBeenCalledTimes(3); \
});"

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
Use axios. Write api.test.ts to test successful response and 404 error. \
Use jest.mocked() instead of casting: jest.mocked(axios.get).mockResolvedValue({ data: userData }); \
NOT: (axios.get as jest.Mock).mockResolvedValue(...) \
Also use: jest.mock('axios'); import axios from 'axios'; const mockedAxios = jest.mocked(axios, { shallow: true });"

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
SUCCESS_RATE=0
if [[ $TOTAL -gt 0 ]]; then
    SUCCESS_RATE=$(( (PASS * 100) / TOTAL ))
fi

echo -e ""
echo -e "${BOLD}╔══════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}║   SEL Smoke Test — النتائج النهائية             ║${NC}"
echo -e "${BOLD}╠══════════════════════════════════════════════════╣${NC}"
printf "${BOLD}║  %-46s ║${NC}\n" "المجموع:    $TOTAL / 12"
printf "${BOLD}║  %-46s ║${NC}\n" "نجح:        $PASS  |  فشل: $FAIL"
printf "${BOLD}║  %-46s ║${NC}\n" "النجاح:     ${SUCCESS_RATE}%"
printf "${BOLD}║  %-46s ║${NC}\n" "المدة:      ${DURATION}s"
echo -e "${BOLD}╠══════════════════════════════════════════════════╣${NC}"
echo -e "${BOLD}║  حسب المهمة:                                     ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════════════════╝${NC}"

echo -e ""
for r in "${RESULTS[@]}"; do
    IFS='|' read -r status id lang timing repairs_str <<< "$r"
    if [[ "$status" == "PASS" ]]; then
        printf "  ${GREEN}✅${NC} %-30s ${BLUE}%-6s${NC} ${BLUE}%s${NC}\n" "$id" "($lang)" "$timing $repairs_str"
    else
        printf "  ${RED}❌${NC} %-30s ${BLUE}%-6s${NC} ${RED}%s${NC}\n" "$id" "($lang)" "$timing $repairs_str"
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
