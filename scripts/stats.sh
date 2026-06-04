#!/usr/bin/env bash
set -euo pipefail

JSON=false
if [[ "${1:-}" == "--json" ]]; then
  JSON=true
fi

RUST_FILES=$(find src -name '*.rs' | wc -l | tr -d ' ')
RUST_LOC=$(find src -name '*.rs' -print0 | xargs -0 cat | wc -l | tr -d ' ')
TRAJ_DIRS=$(find fixtures/trajectories -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
TRAJ_JSON=$(find fixtures/trajectories -type f -name '*.json' ! -name 'index.json' | wc -l | tr -d ' ')
BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo unknown)

if $JSON; then
  cat <<JSON
{
  "version": "v8.5.2",
  "branch": "${BRANCH}",
  "commit": "${COMMIT}",
  "rust_files": ${RUST_FILES},
  "rust_loc": ${RUST_LOC},
  "trajectory_directories": ${TRAJ_DIRS},
  "trajectory_json_files": ${TRAJ_JSON}
}
JSON
else
  echo "SEL Agent Stats"
  echo "---------------"
  echo "version:               v8.5.2"
  echo "branch:                ${BRANCH}"
  echo "commit:                ${COMMIT}"
  echo "rust_files:            ${RUST_FILES}"
  echo "rust_loc:              ${RUST_LOC}"
  echo "trajectory_directories:${TRAJ_DIRS}"
  echo "trajectory_json_files: ${TRAJ_JSON}"
fi
