#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-core}"
BIN="./target/release/sel-agent"

sanitize_log() {
  local logfile="$1"
  python3 - "$logfile" <<'PY'
import pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text(errors="ignore")
text = re.sub(r'\x1b\[[0-9;?]*[ -/]*[@-~]', '', text)
text = text.replace('\r', '')
print(text, end='')
PY
}

need_match_regex() {
  local label="$1"
  local regex="$2"
  local logfile="$3"
  local cleanfile
  cleanfile="$(mktemp)"

  sanitize_log "$logfile" > "$cleanfile"

  if grep -Eq "$regex" "$cleanfile"; then
    echo "✅ $label matched summary regex: $regex"
  else
    echo "❌ $label did not match summary regex: $regex"
    echo "---- $label sanitized log ----"
    cat "$cleanfile"
    rm -f "$cleanfile"
    exit 1
  fi

  rm -f "$cleanfile"
}

echo "==> Building release"
cargo build --release

echo "==> bench all replay"
"$BIN" bench --suite all --replay 2>&1 | tee /tmp/sel_bench_all.log >/dev/null
need_match_regex "bench all" "Passed:[[:space:]]+36/36" /tmp/sel_bench_all.log

echo "==> bench-swe replay"
"$BIN" bench-swe --lang all --replay 2>&1 | tee /tmp/sel_bench_swe.log >/dev/null
need_match_regex "bench-swe" "المجموع:[[:space:]]+30/30" /tmp/sel_bench_swe.log

echo "==> bench-sel-v11 replay"
"$BIN" bench-sel-v11 --replay 2>&1 | tee /tmp/sel_bench_v11.log >/dev/null
need_match_regex "bench-sel-v11" "Core:[[:space:]]+18/18" /tmp/sel_bench_v11.log

if [[ "$MODE" == "full" ]]; then
  echo "==> bench-real-world replay"
  "$BIN" bench-real-world --replay 2>&1 | tee /tmp/sel_bench_real.log >/dev/null
  need_match_regex "bench-real-world" "14/14" /tmp/sel_bench_real.log

  echo "==> smoke replay"
  bash "$(dirname "$0")/sel_smoke_test.sh" "$BIN" --replay 2>&1 | tee /tmp/sel_smoke.log > /dev/null
  if sanitize_log /tmp/sel_smoke.log | grep -Eq '12 / 12|12/12|النجاح:[[:space:]]+100%'; then
    echo "✅ smoke matched 12/12"
  else
    echo "❌ smoke did not match 12/12"
    echo "---- smoke sanitized log ----"
    sanitize_log /tmp/sel_smoke.log
    exit 1
  fi
fi

echo
echo "✅ Regression gate passed ($MODE)"
