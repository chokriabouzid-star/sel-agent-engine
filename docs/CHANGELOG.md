# CHANGELOG

## v1.2.0 — March 2026

### Added
- Multi-file Repair Memory: culprit_file detection from traceback, +8 score in Context Budget
- Token-Aware Repair: ImportError/NodeTestError send file names only
- Repair History Guard: fingerprint-based loop detection
- Goal Validator: rejects vague goals before Planning
- Smart Mutation: 10 operators (was 1), coverage ~90% of files

### Known Limitations
- Protocol Overflow: very large generated files crash JSON plan parser (fix in v1.3)

### Benchmarks
- Stress: 12/12 | avg repairs: 0.9
- Hard: 8/8 | avg repairs: 0.5
- Chaos v1: 8/8 | avg repairs: 0.0
- Chaos v2: 10/10 | avg repairs: 0.0

---

## v1.1.0 — March 2026

### Added
- Context Budget Engine: scores files by relevance (stderr +5, recent +3, test +2)
- Structured Repair Memory: FailedStep tracks command + stderr + failure kind
- Mutation Check: advisory-only (v1.1), tests weak code detection
- NodeTestError classifier: detects Jest/Mocha syntax in Node.js plain tests
- SEL_SUCCESS / SEL_FAILED structured exit codes
- Progress indicator in benchmark_stress.sh

### Benchmarks
- Happy Path: 9/9
- Stress: 12/12 | avg repairs: 0.9
- Real World: 9/9 | avg repairs: 0.3
- Hard: 8/8 | avg repairs: 0.5
