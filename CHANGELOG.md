# CHANGELOG

## [9.0.0] - 2026-06-08

### Added
- Adaptive repair routing wired into the live runtime path
- Smart repair context wired into `do_repairing()`
- Recent edit tracking for smart repair context scoring
- Dependency graph caching across repair attempts
- Expanded Rust dependency parser support for `pub mod`, `use crate::`, `use self::`, and `use super::`

### Changed
- Repair loop escalation now detects same-error streaks and injects stronger guidance
- Repeated constitution violations now force `ForceSourceOnly`
- `RepairCtx::build()` now scans recursively instead of top-level only
- Version consistency uses `Cargo.toml` as single source of truth

### Repository Hygiene
- Removed tracked `.bak*` source snapshots
- Removed ad-hoc patch scripts used to mutate source files directly
- Added maintainer notes for source-of-truth and cleanup policy

### Verified
- `cargo check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test` → 304/304
- `bash scripts/regression_gate.sh core` → PASS
  - bench all replay: 36/36
  - bench-swe replay: 30/30
  - bench-sel-v11 replay: 18/18

# Changelog

## v8.9.0 — Pattern Library (2026-06-06)

### Verified state
- cargo check: pass
- cargo clippy --all-targets --all-features -- -D warnings: pass
- cargo test: 272/272
- bench --suite all --replay: 36/36
- bench-swe --lang all --replay: 30/30
- bench-sel-v11 --replay: 18/18
- bench-real-world --replay: 14/14
- smoke --replay: 12/12
- scripts/regression_gate.sh full: pass

### Wave 3 — Pattern Library
- Added src/pattern_library.rs with PatternLibrary, Pattern, PatternStore, RepairRoute
- Patterns stored in ~/.sel-agent/patterns.json
- Pattern lookup integrated into repair prompt construction
- Pattern outcomes recorded on success and failure
- example_fix extracted from successful plans
- build_pattern_hint: route + guidance + example_fix
- truncate_pattern_example: UTF-8 safe
- infer_language_from_workspace: language detection
- normalize_signature: strips line numbers and paths
- Hardened scripts/regression_gate.sh: sanitize_log + need_match_regex

### Next
- v9.0.0: Adaptive Repair Routing

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
