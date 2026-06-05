#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-core}"
BIN="./target/release/sel-agent"

need_match() {
  local label="$1"
  local expected="$2"
  local logfile="$3"

  if grep -q "$expected" "$logfile"; then
    echo "✅ $label matched $expected"
  else
    echo "❌ $label did not match $expected"
    echo "---- $label log ----"
    cat "$logfile"
    exit 1
  fi
}

echo "==> Building release"
cargo build --release

echo "==> bench all replay"
"$BIN" bench --suite all --replay 2>&1 | tee /tmp/sel_bench_all.log >/dev/null
need_match "bench all" "36/36" /tmp/sel_bench_all.log

echo "==> bench-swe replay"
"$BIN" bench-swe --lang all --replay 2>&1 | tee /tmp/sel_bench_swe.log >/dev/null
need_match "bench-swe" "30/30" /tmp/sel_bench_swe.log

echo "==> bench-sel-v11 replay"
"$BIN" bench-sel-v11 --replay 2>&1 | tee /tmp/sel_bench_v11.log >/dev/null
need_match "bench-sel-v11" "18/18" /tmp/sel_bench_v11.log

if [[ "$MODE" == "full" ]]; then
  echo "==> bench-real-world replay"
  "$BIN" bench-real-world --replay 2>&1 | tee /tmp/sel_bench_real.log >/dev/null
  need_match "bench-real-world" "14/14" /tmp/sel_bench_real.log

  echo "==> smoke replay"
  bash sel_smoke_test.sh "$BIN" --replay 2>&1 | tee /tmp/sel_smoke.log >/dev/null
  if grep -Eq '12 / 12|12/12|النجاح:     100%' /tmp/sel_smoke.log; then
    echo "✅ smoke matched 12/12"
  else
    echo "❌ smoke did not match 12/12"
    cat /tmp/sel_smoke.log
    exit 1
  fi
fi

echo
echo "✅ Regression gate passed ($MODE)"
