#!/usr/bin/env bash
# scripts/canary_check.sh
set +H
set -uo pipefail

BIN="${1:-target/debug/sel-agent}"
TS="$(date +%s)"
PASS=0
FAIL=0
FAILED_NAMES=()

if [ ! -x "$BIN" ]; then
  echo "❌ البرنامج غير موجود أو غير قابل للتشغيل: $BIN"
  exit 2
fi

run_canary() {
  local name="$1"
  local goal="$2"
  local workspace="$3"
  shift 3
  local expect_files=("$@")

  echo "=================================================="
  echo "CANARY: $name"
  echo "workspace: $workspace"
  echo "=================================================="

  rm -rf "$workspace"
  mkdir -p "$workspace"

  "$BIN" run --workspace "$workspace" --goal "$goal"
  local run_exit=$?

  echo "--- فحص القرص المستقل ---"
  local ok=1
  for f in "${expect_files[@]}"; do
    if [ ! -s "$workspace/$f" ]; then
      echo "  ❌ مفقود أو فارغ: $workspace/$f"
      ok=0
    else
      echo "  ✅ موجود: $workspace/$f ($(wc -c < "$workspace/$f") bytes)"
      echo "     --- cat -A ---"
      cat -A "$workspace/$f" | head -5 | sed "s/^/     /"
    fi
  done

  if [ "$ok" -eq 1 ]; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
    FAILED_NAMES+=("$name")
  fi
}

run_canary   "python_basic"   "Create greeter.py with function greet(name:str) -> str returning f'Hello, {name}'. Create test_greeter.py with one pytest test for it."   "/tmp/canary_${TS}_py"   "greeter.py" "test_greeter.py"

run_canary   "typescript_basic"   "Create calc.ts exporting function add(a:number,b:number):number. Create calc.test.ts with one test for it."   "/tmp/canary_${TS}_ts"   "calc.ts" "calc.test.ts"

run_canary   "go_basic"   "Create a Go module named canarymod (run go mod init canarymod). Create add.go in package canarymod with function Add(a,b int) int. Create add_test.go with one table-driven test."   "/tmp/canary_${TS}_go"   "add.go" "add_test.go"

run_canary   "rust_basic"   "Create a Rust library project (run cargo init --lib). In src/lib.rs implement pub fn add(a:i32,b:i32)->i32 with one unit test verifying add(2,3)==5."   "/tmp/canary_${TS}_rs"   "Cargo.toml" "src/lib.rs"

echo "=================================================="
echo "Canary Summary: ${PASS} passed out of $((PASS + FAIL))"
if [ "$FAIL" -gt 0 ]; then
  echo "Failed: ${FAILED_NAMES[*]}"
  exit 1
fi
exit 0
