#!/usr/bin/env bash
set -euo pipefail
OUTPUT="BASELINE.md"

echo "# SEL Agent — Baseline $(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$OUTPUT"

echo "" >> "$OUTPUT"
echo "## Unit Tests" >> "$OUTPUT"
cargo test -- --test-threads=1 2>&1 | tail -5 | tee -a "$OUTPUT"
TEST_COUNT=$(cargo test -- --list 2>&1 | grep -c "test$" || true)
echo "test_count=$TEST_COUNT" >> "$OUTPUT"

echo "" >> "$OUTPUT"
echo "## Replay Suite" >> "$OUTPUT"
cargo build --release -q
./target/release/sel-agent bench --suite all --replay 2>&1 | tail -10 | tee -a "$OUTPUT"

echo "" >> "$OUTPUT"
echo "## Binary Size" >> "$OUTPUT"
SIZE=$(stat -c%s target/release/sel-agent 2>/dev/null || stat -f%z target/release/sel-agent 2>/dev/null || echo "0")
echo "binary_size_bytes=$SIZE" >> "$OUTPUT"

echo "" >> "$OUTPUT"
echo "## Source Lines" >> "$OUTPUT"
find src -name '*.rs' | xargs wc -l | tail -1 | tee -a "$OUTPUT"

echo "" >> "$OUTPUT"
echo "## Git Info" >> "$OUTPUT"
echo "commit=$(git rev-parse --short HEAD)" >> "$OUTPUT"
echo "branch=$(git rev-parse --abbrev-ref HEAD)" >> "$OUTPUT"

echo ""
echo "✅ Baseline saved to $OUTPUT"
cat "$OUTPUT"
