#!/usr/bin/env python3
"""
Canonicalize prompt/response text: remove ISO timestamps, normalize tmp paths and whitespace.
Usage:
  cat file.txt | python3 scripts/canonicalize_prompt.py
"""
import sys, re
txt = sys.stdin.read()
# Remove ISO timestamps like 2023-11-29T12:34:56Z or 2023-11-29T12:34:56+00:00
txt = re.sub(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:Z|[+-]\d{2}:?\d{2})", "<TIMESTAMP>", txt)
# Replace /tmp/sel-bench-xxxxx patterns
txt = re.sub(r"/tmp/sel-bench-\d+", "<TMP_PATH>", txt)
# Replace UUIDs-like tokens (simple heuristic)
txt = re.sub(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}", "<UUID>", txt)
# Normalize whitespace and line endings
txt = re.sub(r"[ \t]+", " ", txt)
txt = re.sub(r"\r\n?", "\n", txt)
txt = "\n".join(line.rstrip() for line in txt.splitlines())
print(txt)
