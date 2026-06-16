# Changelog

All notable changes to SEL Agent are documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/).

---

## [v9.2.6] — 2026-06-14

### Added
- `ExecutionContext.tokens_used: u64` — cumulative token usage per run
- `ExecutionContext.plan_confidence: Option<f32>` — reserved for v11.0 calibration
- `ExecutionReport.total_tokens: u64` — `tokens_in + tokens_out`
- `ExecutionReport.avg_tokens_per_task: u64` — `total_tokens / llm_calls`
- `#[serde(default)]` on new report fields for backward-compatible JSON deserialization
- `evals/feature_impact/plan_risk/RESULTS.md` — impact eval documentation

### Changed
- `agent.rs`: compute and wire `total_tokens` / `avg_tokens_per_task` in both success and failure paths
- `ReportRunInput`: extended with `total_tokens` and `avg_tokens_per_task`

### Gate Results
- `regression_gate.sh full`: ✅ PASS
- `smoke --replay`: 12/12 ✅
- `bench all --replay`: 36/36 ✅
- `bench-swe --replay`: 30/30 ✅
- `bench-sel-v11 --replay`: 18/18 ✅
- `bench-real-world --replay`: 14/14 ✅

---

## [v9.2.5] — 2026-06-12

### Changed
- **decision.rs structural refactor**: split 1400+ line monolith into facade + 5 submodules
  - `decision/checklist.rs` — `pre_repair_checklist` + semantic shortcuts
  - `decision/context_builders.rs` — `build_lang_hint`, `build_ref_context`, `build_workspace_context`
  - `decision/goal.rs` — `GoalClarity`, `validate_goal`, `goal_advisory_hints`
  - `decision/plan_risk.rs` — `evaluate_plan_risk`, `plan_risk_feedback`
  - `decision/validators.rs` — `validate_patch_uniqueness`, `validate_plan_integrity`, `validate_protected_writes`

### Fixed
- Planning guard for `go.mod` — `validate_protected_writes()` rejects `write_file go.mod` early
- tsconfig scaffold — added `"types": ["jest", "node"]` for TS smoke tests
- TS semantic triggers tightened — reduced false repair loops
- Rust E0422 visibility fix — autofix adds `pub` on source only
- `retry.ts` — checklist writes `new Promise` constructor correctly
- `py_dataclass` trajectory — clean trajectory using `run_tests:` not `run:`
- Improved diagnostic messages

### Gate Results
- `smoke --replay`: 12/12 ✅

---

## [v9.2.1] — 2026-06-10

### Added
- Plan Risk Telemetry fields in `ExecutionContext`:
  - `plan_risk_triggered: bool`
  - `plan_risk_reasons: Vec<String>`
  - `replan_count: u32`
  - `commands_before_replan: usize`
- Plan Risk Telemetry fields in `ExecutionReport`:
  - `tokens_in: u64`
  - `tokens_out: u64`
  - `plan_risk_triggered: bool`
  - `replan_count: u64`
  - `plan_risk_reasons: Vec<String>`

### Fixed
- Structural fixes P1/P2/P3

---

## [v9.2.0] — 2026-06-09

### Added
- Plan Risk Telemetry (partial)
- Cost tracking fields

---

## [v9.1.0] — 2026-06-07

### Added
- 7 quality fixes
- Plan Risk connected to planning pipeline
- Prompt quality improvements
- `goal_advisory_hints()`
- `validate_goal()` — hard-fail only for `len < 10`

---

## [v9.0.0] — 2026-06-04

### Added
- Adaptive Repair Routing (8 routes)
- Smart Repair Context (scored file selection + dependency graph)
- Plan Risk Evaluation (`evaluate_plan_risk`)
- Pattern Library (`PatternLibrary` + `RepairRoute` + inference)
- `force_include` guarantee in context builder
- Dependency graph caching in `ExecutionContext`
- Head+tail stderr capture
- Replay mutation safety
- `bench_mode` gate on semantic shortcuts
- Global prompt budget (24k chars)

---

## [v8.9.0] — 2026-06-01

### Added
- Wave 3 Phase 1: Pattern Library

---

## [v8.8.0] — 2026-05-28

### Added
- Wave 1: Reports + Observatory + Regression Gate
- Wave 2: Dependency Graph (Python/TS/Go/Rust)

---

## [v8.5.2] — 2026-05-20

### Fixed
- Stable core + replay environment fix

---

## [v9.3.0] — 2026-06-14

### Added
- `BudgetReport.force_include_dropped: Vec<String>` — tracks unloadable force_include files
- `ExecutionContext.force_include_dropped_count: u64`
- `ExecutionReport.force_include_dropped_count: u64`
- `ExecutionContext`: `context_tokens_total`, `context_tokens_before_total`, `context_files_total`, `context_budget_samples`
- `ExecutionReport`: `avg_context_tokens`, `avg_selected_files`, `context_reduction_pct`
- `compute_score`: stderr_occurrences weight (+2 per extra occurrence, max +6)
- `evals/feature_impact/context_budget/RESULTS.md`

### Changed
- `build_repair_context_block()` returns `(String, BudgetReport)` instead of `String`
- `state_handlers.rs`: accumulates `BudgetReport` into `ExecutionContext` each repair loop
- `agent.rs`: computes averages and wires into `ExecutionReport`

### Evidence Backfill (Wave 1 / Wave 1.5)
- `docs/EVIDENCE_MATRIX.md` added as the claim→evidence registry
- **Constitution enforced** → **PROVEN**
- **Smart Context active** → **PROVEN**
- **Plan Risk connected** → **PROVEN**
- **Telemetry correctness** → **PROVEN**
- Evidence Backfill exposed a real gap in Rule 6:
  - `rm -rf .`
  - `rm -rf ..`
  - `rm -rf *`
  - `rm -rf ~`
  were not blocked before
- Rule 6 was strengthened to block destructive workspace wipes discovered by evidence tests
- `compute_report_telemetry()` extracted into a pure helper and covered by formula tests

### Notes
- 20% context reduction gate: data collection active — gate measured after live repair runs
- All new report fields use `#[serde(default)]` for backward-compatible JSON deserialization

### Gate Results
- `regression_gate core`: ✅ PASS
- `cargo test`: 143/143 ✅
- `bench all --replay`: 36/36 ✅
- `bench-swe --replay`: 30/30 ✅
- `bench-sel-v11 --replay`: 18/18 ✅
