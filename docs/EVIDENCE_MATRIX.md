# Evidence Matrix

## Status legend
- **PROVEN**: السلوك مثبت مباشرة باختبارات evidence
- **PARTIAL**: جزء من الادعاء مثبت، لكن ما زال هناك gap معروف
- **UNVERIFIED**: لم يُكتب له evidence بعد
- **BROKEN**: evidence test كشف خللاً حقيقياً

---

| Claim | Status | Evidence Location | Notes |
|------|--------|-------------------|-------|
| Constitution enforced | **PROVEN** | `src/constitution.rs` | blocks test writes, go.mod overwrite, dangerous commands, network calls; allows safe actions |
| Smart Context active | **PROVEN** | `src/context/builder.rs` | culprit priority, repeated stderr boost, force_include guarantee, dropped tracking, budget metadata |
| Plan Risk connected | **PROVEN** | `src/decision/plan_risk.rs` | existing test writes, delete-file risk, safe plans, feedback lines — all covered |
| Telemetry correctness | **PROVEN** | `src/report.rs`, `src/agent.rs` | schema, roundtrip, backward compat, formula correctness — all proven |
| Dependency Graph used in scoring | **UNVERIFIED** | existing parser/builder tests only | Wave 2 |
| Pattern Library active | **UNVERIFIED** | existing unit tests only | Wave 2 |
| Adaptive Repair Routing active | **UNVERIFIED** | not backfilled yet | Wave 2 |
| Replay mutation safety | **UNVERIFIED** | not backfilled yet | Wave 2 |

---

## Wave 1 outcomes

### PROVEN
- Constitution enforcement
- Smart Context core behavior

### PARTIAL
- Plan Risk
- Telemetry correctness

### Key finding
Evidence Backfill already exposed a real gap:
- Rule 6 did not block `rm -rf .`
- fixed during Wave 1
- Constitution claim moved from weaker/implicit to **PROVEN**

---

## Next steps

### To upgrade Plan Risk → PROVEN
- add a stronger delete-risk evidence test using the exact command/path representation used by runtime planning
- optionally add wiring evidence for propagation into context/report

### To upgrade Telemetry → PROVEN
- extract a small pure helper from `agent.rs` for report metric calculation
- test:
  - `total_tokens = tokens_in + tokens_out`
  - `avg_tokens_per_task`
  - `avg_context_tokens`
  - `context_reduction_pct`
  - `force_include_dropped_count`
