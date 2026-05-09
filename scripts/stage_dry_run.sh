#!/usr/bin/env bash
set -euo pipefail
if [ ! -d ".sel-staging" ]; then
  echo ".sel-staging not found. Populate .sel-staging before running dry run."
  exit 1
fi
TMPDIR=$(mktemp -d)
echo "Copying .sel-staging -> $TMPDIR for dry-run (no change to real workspace)"
rsync -a --delete .sel-staging/ "$TMPDIR/"

# Basic language detection
if [ -f "$TMPDIR/Cargo.toml" ]; then
  echo "Detected Rust project. Running cargo test in staging copy..."
  (cd "$TMPDIR" && cargo test --quiet)
elif [ -f "$TMPDIR/package.json" ]; then
  echo "Detected Node project. Run your local test runner manually inside staging copy:"
  echo "  cd $TMPDIR && npm ci && npm test"
elif [ -f "$TMPDIR/pyproject.toml" ] || ls "$TMPDIR"/*.py >/dev/null 2>&1; then
  echo "Detected Python files. Run pytest manually inside staging copy:"
  echo "  cd $TMPDIR && pytest -q"
else
  echo "No known project type detected. Inspect $TMPDIR manually."
fi
echo "Dry-run completed. Remove $TMPDIR when done or re-run."
