#!/usr/bin/env bash
set -e
BINARY="${1:-./target/release/sel-agent}"
CASE="${2:-PY-01}"
LANG="${3:-python}"
TMP1=$(mktemp); TMP2=$(mktemp)
normalize() {
    sed -E \
      -e 's/[0-9]+\.[0-9]+s\b/TIME/g' \
      -e 's/[0-9]+ ms\b/TIME/g' \
      -e 's/[0-9]+ms\b/TIME/g' \
      -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:\.Z]+/TIMESTAMP/g'
}
"$BINARY" bench-swe --lang "$LANG" --focus "$CASE" --replay 2>&1 | normalize > "$TMP1"
"$BINARY" bench-swe --lang "$LANG" --focus "$CASE" --replay 2>&1 | normalize > "$TMP2"
if diff -q "$TMP1" "$TMP2" > /dev/null 2>&1; then
    echo "PASS: $CASE ($LANG) — replay deterministic"
    rm "$TMP1" "$TMP2"
else
    echo "FAIL: $CASE ($LANG) — replay NOT deterministic"
    diff "$TMP1" "$TMP2" | head -30
fi
