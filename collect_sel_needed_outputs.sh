#!/usr/bin/env bash
set -u

section() {
  printf '\n\n========== %s ==========\n' "$1"
}

dump() {
  local file="$1"
  local start="$2"
  local end="$3"
  if [ -f "$file" ]; then
    section "$file:$start-$end"
    nl -ba "$file" | sed -n "${start},${end}p"
  else
    section "$file"
    echo "MISSING: $file"
  fi
}

echo "PWD: $(pwd)"
echo "ROOT: $(git rev-parse --show-toplevel 2>/dev/null || echo no-git-root)"
echo "BRANCH: $(git branch --show-current 2>/dev/null || echo unknown)"
echo "STATUS:"
git status --short 2>/dev/null || true

section "REMAINING CODE DUMPS"

# 1) CLI / Health / healthcheck parser
dump src/commands/cli.rs 1 260
dump src/commands/health.rs 1 320
dump sel_healthcheck.sh 1 420

# 2) Runner / parsers (we need the rest of runner too)
dump src/executor/parsers.rs 1 260
dump src/executor/runner.rs 1 420

# 3) file_ops full remainder
dump src/executor/file_ops.rs 1 420

# 4) mutation/equivalent tracking
dump src/state_handlers.rs 470 580
dump src/types.rs 1 140
dump src/executor/mutation.rs 1 300

# 5) provider legacy vs live
dump src/provider.rs 1 280
dump src/llm/live.rs 1 420

# 6) optional but useful for the Go autofix path
dump src/executor/core.rs 1 260

section "TARGETED GREPS"

echo
echo "-- runner success logic --"
rg -n "success =|parse_go_tests|parse_pytest|passed|failed|Cannot find module|Node.js test timeout|cargo test timeout|go test timeout|pytest timeout" src/executor/runner.rs || true

echo
echo "-- snapshot rollback / commit usage --"
rg -n "Snapshot::take|snapshot\.rollback|snapshot\.commit|stash|reset --hard|clean -fd" src || true

echo
echo "-- provider usage --"
rg -n "crate::provider::|use crate::provider|pub mod provider|LiveProvider::from_env|clone_shared|primary_name" src || true

echo
echo "-- mutation tracking --"
rg -n "mutation_survival_counts|last_mutation_context|Equivalent|equivalent|Weak|Strong|Skipped" src || true

echo
echo "-- version extraction in healthcheck --"
rg -n "8\.5\.0|8\.4\.1|CARGO_PKG_VERSION|VERSION|cli\.rs|main\.rs|Cargo\.toml" sel_healthcheck.sh src/commands/cli.rs src/main.rs Cargo.toml || true

echo
echo "-- suspicious Unicode slicing left --"
rg -n '&[^ ]*\[\.\.|\.{2}|len\(\)\.min|chars\(\)\.take|k\[\.\.8\]|k\.len\(\) - 4|reason\[\.\.50\]|title\[\.\.' src || true

section "DONE"
echo "Output saved by tee if you used:"
echo "  bash collect_sel_needed_outputs.sh 2>&1 | tee /tmp/sel_needed_outputs.txt"
