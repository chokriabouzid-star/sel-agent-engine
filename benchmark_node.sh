#!/bin/bash
# =============================================================
# SEL Agent — Node.js Benchmark
# 5 tests targeting known LLM failure points in Node.js
# Usage: chmod +x benchmark_node.sh && ./benchmark_node.sh
# =============================================================

AGENT=~/projects/sel-agent-v4/target/release/sel-agent
PASS=0
FAIL=0
RESULTS=()

run_test() {
    local id="$1"
    local name="$2"
    local workspace="$3"
    local goal="$4"

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "▶ $id — $name"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    rm -rf "$workspace"
    result=$($AGENT run --workspace "$workspace" --goal "$goal" --max-repairs 5 2>&1)
    status=$(echo "$result" | grep -o "SEL_SUCCESS\|SEL_FAILED" | tail -1)
    repairs=$(echo "$result" | grep -c "Repair [0-9]*/5")

    if [ "$status" = "SEL_SUCCESS" ]; then
        echo "✅ $id PASSED (repairs: $repairs)"
        PASS=$((PASS + 1))
        RESULTS+=("✅ $id — $name (repairs: $repairs)")
    else
        echo "❌ $id FAILED"
        echo "$result" | grep -E "Error|Cannot|SyntaxError" | head -8
        FAIL=$((FAIL + 1))
        RESULTS+=("❌ $id — $name")
    fi
    sleep 35
}

# ─────────────────────────────────────────────
# N1 — Express + CommonJS + Jest + supertest
# الفخ: LLM يخلط بين import/require أو يضيف "type":"module" بالخطأ
# ─────────────────────────────────────────────
run_test "N1" "Express + CommonJS + Jest + supertest" ~/node-test-n1 \
"Build an Express.js REST API using CommonJS (require, not import).
Requirements:
1. package.json: DO NOT add 'type':'module' — use CommonJS.
2. Express routes: GET /ping returns {pong:true}, POST /echo returns body back.
3. Install: express jest supertest.
4. Jest config in package.json: testEnvironment node.
5. Write Jest tests using supertest for both routes.
6. Test command: jest --runInBand.
7. All files use require() — no import/export ES modules syntax."

# ─────────────────────────────────────────────
# N2 — ESM (import/export) + native fetch
# الفخ: LLM يستخدم node-fetch v2 (CJS) بدل native fetch (Node 18+)
#       أو يستخدم require('node-fetch') مع ESM
# ─────────────────────────────────────────────
run_test "N2" "ESM modules + native fetch (Node 18+)" ~/node-test-n2 \
"Build a Node.js ESM module that fetches data.
Requirements:
1. package.json: set 'type':'module' for ESM.
2. Use native fetch (built into Node 18+) — do NOT install node-fetch.
3. Build WeatherClient class: fetchTemperature(city) fetches from a mock URL.
4. Install: jest (use --experimental-vm-modules for ESM jest).
5. package.json test script: 'NODE_OPTIONS=--experimental-vm-modules jest'.
6. Mock fetch using jest.spyOn(global, 'fetch').
7. Write tests: successful fetch returns temperature, failed fetch throws error.
8. All files use import/export syntax."

# ─────────────────────────────────────────────
# N3 — JWT with jsonwebtoken + bcrypt
# الفخ: callback vs promise API في bcrypt
#       LLM قد يخلط bcrypt مع bcryptjs
# ─────────────────────────────────────────────
run_test "N3" "JWT + bcrypt password hashing" ~/node-test-n3 \
"Build authentication utilities in Node.js (CommonJS).
Requirements:
1. package.json: CommonJS (no 'type':'module').
2. Install: jsonwebtoken bcrypt jest.
3. Use bcrypt (NOT bcryptjs) with async/await (bcrypt.hash and bcrypt.compare return Promises).
4. AuthService class:
   - hashPassword(plain): returns hashed string using bcrypt, saltRounds=10.
   - verifyPassword(plain, hash): returns boolean.
   - generateToken(payload, secret, expiresIn='1h'): returns JWT string.
   - verifyToken(token, secret): returns decoded payload or throws.
5. Write Jest tests covering all 4 methods.
6. Test: hash then verify correct password = true, wrong password = false."

# ─────────────────────────────────────────────
# N4 — async/await + Promise.all + error handling
# الفخ: unhandled rejections، missing try/catch، Promise.all fail-fast
# ─────────────────────────────────────────────
run_test "N4" "async/await + Promise.all + proper error handling" ~/node-test-n4 \
"Build async task processor in Node.js (CommonJS).
Requirements:
1. package.json: CommonJS.
2. Install: jest.
3. TaskProcessor class:
   - processAll(tasks): runs all tasks concurrently with Promise.all, returns results array.
   - processWithTimeout(task, ms): rejects with TimeoutError if task takes longer than ms.
   - processWithRetry(task, maxRetries): retries on failure up to maxRetries times.
4. Each task is an async function () => result.
5. Custom TimeoutError class extending Error.
6. Write Jest tests:
   - processAll with 3 tasks returns all results.
   - processWithTimeout: fast task passes, slow task throws TimeoutError.
   - processWithRetry: task failing twice then succeeding on third attempt.
7. Run jest --runInBand."

# ─────────────────────────────────────────────
# N5 — TypeScript + tsc + ts-jest
# الفخ: tsconfig.json settings خاطئة
#       LLM قد ينسى ts-jest config في jest.config
# ─────────────────────────────────────────────
run_test "N5" "TypeScript + ts-jest + proper tsconfig" ~/node-test-n5 \
"Build a TypeScript utility library.
Requirements:
1. package.json with TypeScript setup.
2. Install: typescript ts-jest @types/jest jest.
3. tsconfig.json: target ES2020, module commonjs, strict true, outDir ./dist.
4. jest.config.js using ts-jest preset: module.exports = { preset:'ts-jest', testEnvironment:'node' }.
5. Build Calculator class in src/calculator.ts:
   - add, subtract, multiply, divide methods (divide throws DivisionByZeroError if divisor is 0).
   - Custom DivisionByZeroError extends Error.
   - All methods typed with number parameters and return types.
6. Write tests in src/calculator.test.ts covering all methods including error case.
7. Test command: jest."

# ─────────────────────────────────────────────
# RESULTS SUMMARY
# ─────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════╗"
echo "║     Node.js Benchmark Results            ║"
echo "╚══════════════════════════════════════════╝"
for r in "${RESULTS[@]}"; do
    echo "  $r"
done
echo ""
echo "  PASSED: $PASS / $((PASS + FAIL))"
echo ""
