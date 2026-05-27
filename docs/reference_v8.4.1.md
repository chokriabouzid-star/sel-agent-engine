# SEL Agent — Reference Documentation
## Version 8.4.1

---

## 1. Executive Summary

SEL Agent is an autonomous software engineering agent written in Rust.

Core components:
- LLM-driven planning
- Safe command execution
- Structured repair loops
- Snapshot-based rollback
- Record / Replay trajectories
- Internal benchmarks (SELBench)

### Current Status

| Component | Status |
|-----------|--------|
| `cargo test` | ✅ 86 passed |
| `cargo build --release` | ✅ |
| SELBench v1.0 | ✅ 10/10 |
| SELBench v1.1-rc Live | ✅ 17/18 (94.4%) |
| SELBench v1.1-rc Replay | ✅ 18/18 (100%) |
| avg repairs/case (live) | 0.44 |
| avg repairs/case (replay) | 0.28 |

> SEL Agent is no longer a prototype.
> The architecture is stable and consistent across Python, Go, Rust, and TypeScript.

---

## 2. What Changed in v8.4.1

### Phase A — Tightening

1. Protocol enforcement tightened
   - `run_tests` before `done` now enforced in planning prompt
   - Reduced `PLAN ERROR: Plan missing run_tests before done`

2. Canonical pip behavior
   - Uses `venv/bin/pip install <pkg>`
   - Avoids `python3 -m pip` and absolute paths

3. Cargo.toml policy
   - `write_file` now allowed on `Cargo.toml`
   - `Cargo.lock`, `go.mod`, `go.sum` remain protected

4. Patch sanitization
   - `sanitize_code()` applied to file content, search, and replace
   - Fixes unicode quote issues in Rust files

5. Error history
   - Replaced `previous_error: Option<String>`
   - Now `error_history: Vec<String>` (full history)
   - Better loop detection

6. Executor contract
   - `patch_file` fails correctly when search block missing
   - No silent success on missing search

### SELBench v1.0 (new)
- 10 internal cases across logic / dependency / quickfix / multifile / sanitizer / mutation

### SELBench v1.1-rc (new)
- 18 core cases across 4 languages
- Live: 17/18 (94.4%)
- Replay: 18/18 (100%)

---

## 3. Architecture

### State Machine
Planning → Executing → Repairing → Done
↓
WaitingForUserInput
↓
Failed

text


### File Layout
src/
├── main.rs # Entry point + CLI dispatch
├── agent.rs # State machine
├── state_handlers.rs # Planning / Executing / Repairing
├── repair_strategy.rs # Repair prompts + loop detection
├── decision.rs # Validation + checklist + replan
├── protocol.rs # JSON parsing → commands
├── types.rs # Core types
├── constitution.rs # Hard safety rules
├── diagnostic.rs # Error hint extraction
├── memory.rs # Quick fixes + failure memory
├── snapshot.rs # git stash rollback
├── scaffold_engine.rs # Project env setup
├── bench_sel.rs # SELBench v1.0 + v1.1-rc
├── bench_swe.rs # Mini SWE-Bench (30 cases)
├── bench_realworld.rs # Real-world benchmark
├── executor/
│ ├── core.rs # SafeExecutor + shell policy
│ ├── file_ops.rs # patch / write / append / read
│ ├── runner.rs # run_tests per language
│ ├── compile.rs # compile checks
│ ├── mutation.rs # mutation testing
│ └── sanitizers.rs # autofix / sanitizers
└── llm/
├── live.rs # Provider orchestration
├── replay.rs # Replay mode
├── record.rs # Trajectory recording
└── key_pool.rs # Key rotation

text


---

## 4. Repair Engine

### Fields

```rust
// In Agent:
error_history: Vec<String>
repair_fingerprints: Vec<u64>

// In ExecutionContext:
repair_attempts: u8
failed_steps: Vec<FailedStep>
current_failure_kind: Option<FailureKind>
mutation_survival_counts: HashMap<String, u8>
bench_mode: bool
Loop Detection (3 levels)
Consecutive identical errors
Same 120-char prefix between last two errors
Current error seen previously in history
Heuristic: attempt > 3 with no progress
Escalation Strategy
Attempt	Condition	Strategy
1	any	Fix source files only
2	no loop	Focus on file + function
2	loop	Fix only the one broken line
3+	loop	Rewrite from scratch
3+	no loop	Read error, fix root cause
> max	any	Give up
Pre-Repair Checklist (no LLM)
Missing run_tests → inject once
Python NameError → auto-import stdlib
Rust E0762 → unicode sanitize
All patch_file failures → convert to write_file
5. Safety Model
Constitution Rules
Never modify test files
No empty writes
No null bytes in text files
No system path writes
No overwrite of go.mod
No destructive shell commands
No network fetches during tests
Protocol Rules (v8.4+)
run_tests must precede done
pip: use venv/bin/pip install <pkg>
Cargo.toml deps: prefer write_file with complete content
No duplicate write_file for same path in one plan
write_file Protection
File	Status
Cargo.toml	✅ writable
Cargo.lock	❌ protected
go.mod	❌ protected
go.sum	❌ protected
6. Scaffold Engine
Language	Setup
Python	venv + pytest + extra deps
Go	go mod init sel_tmp
Rust	Cargo.toml baseline
TypeScript	package.json + tsconfig.json + jest (pinned)
TypeScript pinned versions:

typescript: 5.3.3
ts-jest: 29.1.1
jest: 29.7.0
@types/jest: 29.5.11
7. SELBench v1.0
Cases (10)
ID	Title	Category
SB-01	Pagination Off-by-One	logic
SB-02	Mutable Default Argument	logic
SB-03	Missing Python Dependency	dependency
SB-04	Missing Go Import	quickfix
SB-05	Rust Missing Crate	dependency
SB-06	Multi-file Wrong Function Name	multifile
SB-07	Rust Wrong Function Signature	multifile
SB-08	NameError Auto Import	pre_repair
SB-09	Rust Unicode Quote Sanitizer	sanitizer
SB-10	Mutation Resistance	mutation
Results
text

10 / 10
avg repairs ≈ 0.1–0.3
8. SELBench v1.1-rc
Cases (18 Core + 2 System)
ID	Title	Language	Category
RC-01	Python Async Missing Await	python	async
RC-02	Python Dataclass Default Factory	python	datamodel
RC-03	Python Circular Import	python	multifile
RC-04	Python Pathlib Return Type	python	typing
RC-05	Go Nil Map Assignment	go	runtime
RC-06	Go Interface Signature Mismatch	go	interface
RC-07	Go Unused Import	go	quickfix
RC-08	Go Empty Slice Guard	go	edge-case
RC-09	Rust Move After Use	rust	ownership
RC-10	Rust Missing Module Export	rust	module
RC-11	Rust Missing Clone Derive	rust	trait
RC-12	Rust Iterator &&T Deref	rust	iterator
RC-13	Rust Borrow Conflict	rust	borrow
RC-14	TypeScript Promise.all Missing	typescript	async
RC-15	TypeScript Export Mismatch	typescript	export
RC-16	TypeScript Nullish vs Falsy	typescript	logic
RC-17	Python Config Env Alignment	python	multifile
RC-18	Rust Module Rename Re-export	rust	multifile
RC-S1	Replay Determinism	python	system
RC-S2	Auto-rerecord Healing	python	system
Results
Mode	Score	avg repairs
Live	17/18 (94.4%)	0.44
Replay	18/18 (100%)	0.28
By Language (Replay — final)
Language	Score
Python	5/5
Go	4/4
Rust	6/6
TypeScript	3/3
Only failing case (Live mode)
RC-03 — Python Circular Import

Structural design issue, not a trivial patch
Requires dependency graph restructuring
Succeeds in replay mode (trajectory available)
Known limitation — not a blocker for v8.4.1
9. CLI Reference
Core
Bash

sel-agent run \
  --workspace <path> \
  --goal '<goal>' \
  --max-repairs 3 \
  --record | --replay | --rerecord
Mini SWE-Bench
Bash

sel-agent bench-swe \
  --lang all|python|go|rust|typescript \
  --focus "PY-01,GO-02" \
  --max-repairs 5 \
  --delay 8 \
  --record | --replay | --rerecord
SELBench v1.0
Bash

sel-agent bench-sel --max-repairs 5 --delay 3
sel-agent bench-sel --focus "SB-01,SB-05"
sel-agent bench-sel --record
sel-agent bench-sel --replay
SELBench v1.1-rc
Bash

sel-agent bench-sel-v11 --max-repairs 5 --delay 5
sel-agent bench-sel-v11 --focus "RC-01,RC-05"
sel-agent bench-sel-v11 --include-system
sel-agent bench-sel-v11 --record
sel-agent bench-sel-v11 --replay --rerecord
Stress
Bash

sel-agent stress --cases 8 --delay 5
sel-agent stress --record
Utilities
Bash

sel-agent health
sel-agent scan --workspace .
sel-agent reset-providers
10. Environment Variables
Bash

# Groq (up to N keys)
GROQ_API_KEY_1=gsk_...
GROQ_API_KEY_2=gsk_...

# Gemini
GEMINI_API_KEY_1=AIza...

# Cerebras
CEREBRAS_API_KEY_1=csk-...

# OpenRouter / SEL
OPENROUTER_API_KEY=sk-or-v1-...
SEL_API_KEY=sk-...

# GitHub
GITHUB_TOKEN=ghp_...

# Custom model overrides
GROQ_MODEL=llama-3.3-70b-versatile
GEMINI_MODEL=gemini-2.0-flash
CEREBRAS_MODEL=qwen-3-235b-a22b-instruct-2507
OPENROUTER_MODEL=qwen/qwen3-coder:free

# Bench mode (disables EXPLAIN MODE)
SEL_BENCH_MODE=1

# Observatory (optional)
SEL_OBSERVATORY=http://localhost:8777
11. Build & Test
Bash

# Check
cargo check

# Full test suite
cargo test

# Release build
cargo build --release

# Quick smoke test
./target/release/sel-agent bench-sel --focus "SB-01" --replay --delay 0

# Full SELBench v1.1-rc replay
./target/release/sel-agent bench-sel-v11 --replay --delay 0
12. Known Limitations
Issue	Severity	Notes
Python circular import	Medium	Structural, not trivial
TypeScript test counter display	Low	Reports wrong count, pass/fail is correct
Telemetry fields not surfaced	Low	Struct defined, not populated
Version strings inconsistency	Fixed in v8.4.1	
13. Readiness Assessment
Domain	Rating
Planning	8.5 / 10
Execution	9 / 10
Repair Engine	9 / 10
Rust Repair	9 / 10
Go Repair	9.5 / 10
TypeScript Repair	8.5 / 10
Python Repair	8 / 10
Snapshot Safety	9 / 10
Protocol Enforcement	9 / 10
Benchmark Confidence	9 / 10
14. Conclusion
SEL Agent v8.4.1 is architecturally stable and proven across 28 benchmark
cases spanning Python, Go, Rust, and TypeScript.

SELBench v1.0: 10/10
SELBench v1.1-rc Live: 17/18 (94.4%)
SELBench v1.1-rc Replay: 18/18 (100%)
The single known limitation is Python circular import restructuring,
which is a structural design challenge rather than a simple bug.

The system is ready for controlled production experimentation
on small-to-medium engineering tasks.

Generated: v8.4.1 — SELBench v1.1-rc certified
