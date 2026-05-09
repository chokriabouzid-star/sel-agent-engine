#!/usr/bin/env bash
set -euo pipefail
if [ ! -d ".sel-staging" ]; then
  echo ".sel-staging not found. Nothing to apply."
  exit 1
fi
echo "Applying .sel-staging -> workspace. Make sure you ran dry-run and tests passed."
rsync -a --delete .sel-staging/ ./ 
echo "Applied staging changes to workspace."
