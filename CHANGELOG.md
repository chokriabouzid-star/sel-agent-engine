# Changelog

## v8.8.0 — observability + dependency graph (2026-06-04)

### Verified state
- cargo check: pass
- cargo clippy --all-targets --all-features -- -D warnings: pass
- cargo test: 240/240
- scripts/regression_gate.sh full: pass
- bench --suite all --replay: 36/36
- bench-swe --lang all --replay: 30/30
- bench-sel-v11 --replay: 18/18
- bench-real-world --replay: 14/14
- smoke --replay: 12/12

### Wave 1 — Observability
- Added ExecutionReport backend
- Added report writer to ~/.sel-agent/reports/
- Added report --latest and --summary CLI commands
- Added observatory TUI
- Added local regression gate script

### Wave 2 — Dependency Graph
- Added dependency graph core model (DependencyGraph, FileNode, DependencyEdge)
- Added Python / TypeScript / Go / Rust parsers
- Added workspace graph builder
- Added graph-aware context scoring in context/builder.rs

### Next
- v8.9.0: Pattern Library

## v8.5.2 — stabilized working tree (2026-06-04)

### Verified state
- cargo check: pass
- cargo clippy --all-targets --all-features -- -D warnings: pass
- bench --suite all --replay: 36/36
- bench-swe --lang all --replay: 30/30
- bench-sel-v11 --replay: 18/18
- bench-real-world --replay: 14/14
- smoke --replay: 12/12

### Recent fixes
- Fixed Python replay environment mismatch in `src/executor/runner.rs`
- Replay now restores cached Python venv when trajectory requires `venv/bin/pytest`
- Fixed UTF-8 safe truncation in benchmark output (`src/bench_sel.rs`)
- Stabilized replay behavior for real-world Python benchmark cases

### Next planned milestones
- v8.6.0: Unified Reports + Observatory
- v8.6.1: Stability Layer + trajectory manifest
- v8.8.0: Dependency Graph
