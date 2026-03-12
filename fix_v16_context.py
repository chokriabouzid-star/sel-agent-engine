#!/usr/bin/env python3
"""
SEL Agent v1.6 — context.rs patch
Adds start_time: Option<std::time::Instant> to ExecutionContext
and initializes it in agent.rs run()
"""

import shutil, sys, re
from pathlib import Path

BASE   = Path.home() / "projects/sel-agent-v4/src"
CTX    = BASE / "context.rs"
AGENT  = BASE / "agent.rs"

def backup(p): shutil.copy2(p, p.with_suffix(p.suffix + ".bak2")); print(f"  ✓ backup → {p.name}.bak2")
def read(p): return p.read_text(encoding="utf-8")
def write(p, s): p.write_text(s, encoding="utf-8")

# ── 1. context.rs ──────────────────────────────────────────
def patch_context():
    print("\n[1/2] context.rs — add start_time field")
    src = read(CTX)
    backup(CTX)

    if "start_time" in src:
        print("  ⚠️  already present — skipping")
        return

    # Find the struct definition and add the field
    # Strategy: find the last field before closing } of ExecutionContext
    # Insert after the first field line inside the struct

    # Add use statement if not present
    if "use std::time::Instant" not in src:
        src = "use std::time::Instant;\n" + src

    # Find closing brace of struct — add field just before it
    # We look for the pub struct ExecutionContext block
    struct_match = re.search(r'(pub struct ExecutionContext\s*\{[^}]*?)(\})', src, re.DOTALL)
    if not struct_match:
        print("  ✗ could not find 'pub struct ExecutionContext' — check context.rs manually")
        print("    Add this field manually:")
        print("      pub start_time: Option<Instant>,")
        return

    old_block = struct_match.group(0)
    # Add field before closing brace
    new_block = struct_match.group(1) + "    pub start_time: Option<Instant>,\n" + struct_match.group(2)
    src = src.replace(old_block, new_block, 1)

    # Also patch Default/new() impl if exists — set start_time: None
    if "start_time: None" not in src:
        # Find impl block for ExecutionContext or Default
        src = re.sub(
            r'(impl(?:\s+Default\s+for)?\s+ExecutionContext\s*\{.*?fn\s+(?:default|new)\s*\([^)]*\)\s*(?:->[^{]*)?\{)',
            lambda m: m.group(0),
            src,
            flags=re.DOTALL
        )
        # Simpler: just append Default impl if missing
        if "impl Default for ExecutionContext" not in src and "fn default()" not in src:
            # Find struct fields to build default
            pass  # will handle via agent.rs initialization instead

    write(CTX, src)
    print("  ✓ start_time: Option<Instant> added to ExecutionContext")

# ── 2. agent.rs ────────────────────────────────────────────
def patch_agent():
    print("\n[2/2] agent.rs — initialize start_time at run() start")
    src = read(AGENT)
    backup(AGENT)

    if "start_time" in src:
        print("  ⚠️  start_time already referenced — skipping agent.rs patch")
        return

    # Find the pub async fn run( or pub fn run( opening
    # and add self.ctx.start_time = Some(Instant::now()); as first statement
    run_match = re.search(
        r'(pub async fn run\([^)]*\)[^{]*\{)',
        src
    )
    if not run_match:
        # try non-async
        run_match = re.search(r'(pub fn run\([^)]*\)[^{]*\{)', src)

    if not run_match:
        print("  ✗ could not find run() method — add manually:")
        print("    self.ctx.start_time = Some(std::time::Instant::now());")
        return

    old = run_match.group(0)
    new = old + "\n        self.ctx.start_time = Some(std::time::Instant::now());"
    src = src.replace(old, new, 1)

    write(AGENT, src)
    print("  ✓ start_time initialized at run() start")

# ── main ───────────────────────────────────────────────────
def main():
    print("=" * 50)
    print("SEL Agent v1.6 — fix_v16_context.py")
    print("=" * 50)

    missing = [p for p in [CTX, AGENT] if not p.exists()]
    if missing:
        for m in missing: print(f"  ✗ not found: {m}")
        sys.exit(1)

    patch_context()
    patch_agent()

    print("\n" + "=" * 50)
    print("✅ Done. Now run:")
    print()
    print("  cd ~/projects/sel-agent-v4")
    print("  cargo build --release 2>&1 | tail -20")
    print("=" * 50)

if __name__ == "__main__":
    main()
