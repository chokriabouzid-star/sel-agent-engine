# CHANGELOG

## v9.3.4 (2026-06-26)

### Fixed
- Replaced runtime SEL_* test-write env vars with explicit `SafeExecutor` state
- Hardened replay determinism: npm install blocked in replay when node_modules present
- Fixed false positive in `validate_rust_bootstrap_plan` for TypeScript projects
- Added `python/class-no-init` diagnostic for `TypeError: Class() takes no arguments`

### Improved
- Unified telemetry computation via `compute_report_telemetry()` in agent.rs

### Updated Trajectories
- TS-06: re-recorded (80pts, 1 repair)
- `run_write_a_python_user_dataclass_`: re-recorded after diagnostic improvement
- Replay corpus: restored from backup (resolved TRAJECTORY_INCOMPLETE errors)

### Gates
bench all: 36/36 ✅
bench-swe: 30/30 ✅
bench-sel-v11: 18/18 ✅
bench-real-world: 14/14 ✅
smoke: 12/12 ✅


---

## v9.3.3 (2026-06-22)

### Added
- `MissingTests` failure kind (v9.3.3): cargo/pytest ran 0 tests → treated as failure
- Dormant sanitizers wired into write/patch flows
- Constitution `check_command` wired into executor safety check

---

## v9.3.1 (2026-06-16)

### Added
- Evidence Backfill Wave 1/1.5 — 4 core claims PROVEN
- Rule 6 hardening: blocks `rm -rf .` / `..` / `*` / `~`
- `compute_report_telemetry()` with 7 evidence tests

---

## v9.3.0 (2026-06-10)

### Added
- Context Budget Telemetry: avg_context_tokens, context_reduction_pct, force_include_dropped_count
- stderr_occurrences weight in repair context scoring
- force_include guarantee + dropped tracking

---

## v9.2.x (2026-06-01 — 2026-06-09)

### v9.2.6
- tokens_used, plan_confidence fields
- total_tokens, avg_tokens_per_task telemetry

### v9.2.5
- decision.rs refactored → facade + 5 submodules

### v9.2.1
- Plan Risk Telemetry: plan_risk_triggered, replan_count, plan_risk_reasons
- 7 quality fixes across multiple modules

### v9.2.0
- Plan Risk Evaluation connected

---

## v9.0.0 — v9.1.0

### v9.1.0
- Prompt Quality + Goal Clarity improvements

### v9.0.0
- Adaptive Repair Routing (8 routes)
- Smart Repair Context + Pattern Library
- Dependency Graph integration
