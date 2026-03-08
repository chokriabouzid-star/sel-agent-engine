# SEL Agent v1.2 — Release Documentation

> Self-Executing LLM Agent — State Machine Engine
> Released: March 2026

---

## What's New in v1.2

### 1. Multi-file Repair Memory
FailedStep tracks culprit_file extracted from error traceback.
Culprit file receives +8 score in Context Budget Engine.

Supports: Python, pytest, Rust, Go, Node.js tracebacks.

Debug: 🎯 Culprit files: ["service.py"]

### 2. Token-Aware Repair
ImportError and NodeTestError send file names only instead of full contents.
Reduces token usage 60-80% for dependency errors.

Debug: ⚡ Token-Aware: sending file names only (2 files)

### 3. Repair History Guard
repair_fingerprints: Vec<u64> prevents LLM repeating same fix.
Adds warning to prompt when loop detected.

### 4. Goal Validator
validate_goal() runs before Planning. Rejects:
- Goals under 20 characters
- Goals with no test requirement
- Goals with ambiguous test values

Output: SEL_FAILED: Goal has no test requirement

### 5. Smart Mutation
apply_mutation() tries 10 operators in order:
  == -> !=
  != -> ==
  >  -> 
  <  -> >
  >= -> <=
  <= -> >=
  return True  -> return False
  return False -> return True
  +  -> -
  -  -> +

Coverage: ~20% of files (v1.1) -> ~90% of files (v1.2)

---

## Known Limitation

Protocol Overflow: very large generated files embedded in JSON plan
cause parse failure at ~2500 chars.

Fix scheduled for v1.3: Protocol Resilience (auto-retry with simplified plan).

---

## Benchmark Results

Stress (12 tasks):     12/12 | avg repairs: 0.9
Hard (8 tasks):         8/8  | avg repairs: 0.5
Chaos v1 (8 tasks):     8/8  | avg repairs: 0.0
Chaos v2 (10 tasks):   10/10 | avg repairs: 0.0

Notable passes:
- Thread-safe bank account (50 concurrent threads)
- LRU Cache O(1) with eviction
- Job Scheduler with precise timing
- Persistent KV store (disk + restart)

---

## v1.1 vs v1.2

| Feature              | v1.1 | v1.2 |
|----------------------|------|------|
| Culprit detection    |  No  | Yes  |
| Token-Aware Repair   |  No  | Yes  |
| Repair loop guard    |  No  | Yes  |
| Goal validation      |  No  | Yes  |
| Mutation operators   |   1  |  10  |
| Mutation coverage    | ~20% | ~90% |
| Protocol resilience  |  No  |  No (v1.3) |

---

## v1.3 Roadmap

- Protocol Resilience: retry on JSON plan parse failure
- Mutation Enforcement: weak tests trigger repair cycle
- FlaskConcurrency classifier: LookupError: flask.app_ctx
- --dry-run CLI flag
