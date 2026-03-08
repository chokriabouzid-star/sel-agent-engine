# SEL Agent — Master Reference Document
> Last updated: March 2026 — after v1.2 completion
> Purpose: Full project context for any new AI session

---

## 1. What is SEL Agent?

SEL (Self-Executing LLM) Agent is a **Rust-based autonomous coding agent**.

Given a natural language goal, it:
1. Calls an LLM to generate a structured execution plan (JSON)
2. Executes the plan (create files, install deps, run tests)
3. If tests fail — repairs automatically (up to N times)
4. Reports SEL_SUCCESS or SEL_FAILED

**It is NOT a chat assistant. It is a state machine that executes code autonomously.**

Single developer: Chokri Bouzid (sole developer and decision-maker).

---

## 2. Technology Stack

| Layer | Choice |
|---|---|
| Language | Rust (stable) |
| LLM Backend | Groq API (llama-3.3-70b) |
| Test runner | pytest (Python), cargo test (Rust) |
| Build | cargo build --release |
| Binary | ~/projects/sel-agent-v4/target/release/sel-agent |

---

## 3. Project Location

```
~/projects/sel-agent-v4/
├── src/
│   ├── main.rs         — CLI entry point
│   ├── agent.rs        — State machine core
│   ├── executor.rs     — Command execution + mutation check
│   ├── context.rs      — Context Budget Engine
│   ├── types.rs        — FailureKind + FailedStep + extract_culprit()
│   ├── llm.rs          — LLM API calls + retry layer
│   └── protocol.rs     — JSON plan parsing
├── docs/
│   ├── CHANGELOG.md
│   └── SEL_AGENT_V1.2.md
├── benchmark.sh        — Happy path tests
├── benchmark_stress.sh — Stress tests (deliberate errors)
├── Cargo.toml
└── CHANGELOG.md
```

---

## 4. How to Run

```bash
cd ~/projects/sel-agent-v4
cargo build --release

./target/release/sel-agent run \
  --workspace ~/my-project \
  --goal "Create add(a,b) in calc.py. test_calc.py with pytest. Install pytest. Run tests." \
  --max-repairs 3
```

Output:
```
SEL_SUCCESS   — all tests passed
SEL_FAILED: <reason>   — could not recover
```

Environment variables:
```bash
GROQ_API_KEY=gsk_...   # required
SEL_DEBUG=1            # optional: verbose output
```

---

## 5. Architecture — State Machine

```
Planning
   │
   ▼
Executing ──► if test fails ──► Repairing ──► Executing (retry)
   │                                │
   │                          (max repairs reached)
   ▼                                ▼
Done (success)              Done (failed)
```

State transitions in `src/agent.rs`.

---

## 6. Core Concepts

### 6.1 Plan Format (JSON)
The LLM returns a plan like:
```json
{
  "version": "1.0",
  "commands": [
    {"type": "run", "command": "python3 -m venv venv"},
    {"type": "run", "command": "venv/bin/pip3 install pytest"},
    {"type": "write_file", "path": "calc.py", "content": "..."},
    {"type": "run_tests", "path": "test_calc.py"},
    {"type": "done", "message": "All tests passed"}
  ]
}
```

Command types: `run`, `write_file`, `run_tests`, `done`

### 6.2 FailureKind (src/types.rs)
```rust
pub enum FailureKind {
    ImportError,
    AssertionError,
    SyntaxError,
    TypeError,
    NodeTestError,
    Unknown,
}
```
Used to classify errors before repair. Determines repair strategy.

### 6.3 Context Budget Engine (src/context.rs)
Scores files for inclusion in repair prompt (token budget):
```
culprit_file  +8   (highest — extracted from traceback)
stderr mention +5
recently modified +3
test file      +2
small file     +1
```

### 6.4 Mutation Check (src/executor.rs)
After tests pass, SEL mutates the source file and re-runs tests.
- `✅ Tests are solid` — tests caught the mutation (strong tests)
- `⚠️ Tests are weak` — tests passed on broken code (weak tests)
- Advisory-only in v1.2 (does not block success)

---

## 7. Version History

### v1.0 — Foundation
- Basic state machine (Plan → Execute → Repair → Done)
- LLM plan generation + JSON parsing
- File write + shell command execution
- pytest integration

### v1.1 — Intelligence Layer
- **Context Budget Engine**: scores files by relevance for repair prompt
- **Structured Repair Memory**: FailedStep tracks command + stderr + failure kind
- **Mutation Check** (advisory): detects weak tests after success
- **NodeTestError classifier**: detects Jest/Mocha syntax in plain Node.js
- **SEL_SUCCESS / SEL_FAILED**: structured exit codes
- **LLM retry layer**: handles Groq 503 errors automatically

Benchmarks: Happy 9/9 | Stress 12/12 | Real World 9/9 | Hard 8/8

### v1.2 — Repair Quality
- **Multi-file Repair Memory**: culprit_file detection from traceback
- **Token-Aware Repair**: ImportError/NodeTestError send file names only
- **Repair History Guard**: fingerprint-based loop detection
- **Goal Validator**: rejects vague goals before Planning
- **Smart Mutation**: 10 operators (was 1), coverage ~90% of files

Benchmarks: Stress 12/12 | Hard 8/8 | Chaos v1 8/8 | Chaos v2 10/10

---

## 8. Design Decisions (Important)

| Decision | Reason |
|---|---|
| Mutation Check is advisory (not blocking) | Blocking causes infinite repair loops |
| Goal Validator is silent (no questions) | Preserves autonomous philosophy |
| Token-Aware only for Import/NodeTest | Other errors need full file context |
| Rust (not Python) | Performance + correctness for state machine |
| Groq (not OpenAI) | Speed + free tier availability |

---

## 9. Known Limitations (as of v1.2)

### 9.1 Protocol Overflow ← scheduled for v1.3
**Symptom:**
```
❌ JSON parse error: expected `,` or `}` at line 7 column 2633
SEL_FAILED: JSON parse error
```
**Cause:** LLM generates very large file content (e.g., JSON parser from scratch) embedded inside the JSON plan. Special characters break the plan parser.

**Planned fix (v1.3):** Protocol Resilience — auto-retry with simplified instructions when plan parsing fails.

### 9.2 Self-Consistent Testing
LLM writes both code and tests. Tests may be designed to pass with the generated code (not adversarial). Mutation Check partially addresses this.

### 9.3 FlaskConcurrency classifier missing
`LookupError: flask.app_ctx` is classified as `Unknown`. Scheduled for v1.3.

### 9.4 Groq 503 instability
External API instability causes occasional benchmark variance. Not a code issue — the retry layer handles it.

---

## 10. v1.3 Roadmap

| Feature | Description | Priority |
|---|---|---|
| Protocol Resilience | Retry planning on JSON parse failure | 1 |
| Mutation Enforcement | Weak tests → repair cycle (not just warning) | 2 |
| FlaskConcurrency classifier | `LookupError: flask.app_ctx` → new FailureKind | 3 |
| `--dry-run` CLI flag | Preview plan without executing | 4 |

---

## 11. Benchmark Infrastructure

### benchmark_stress.sh
12 stress tests with deliberate errors (wrong imports, bad logic, syntax errors).
Each test has `sleep 10` between runs to avoid Groq rate limits.

### How to run benchmarks:
```bash
# Unit tests
cargo test

# Stress benchmark
bash ~/projects/sel-agent-v4/benchmark_stress.sh

# Custom test
rm -rf ~/test-workspace
./target/release/sel-agent run \
  --workspace ~/test-workspace \
  --goal "..." \
  --max-repairs 3
```

---

## 12. Code Delivery Preferences

When writing Rust patches in a new session:
- Use `cat > file << 'EOF'` format for simple replacements
- Use Python scripts for multi-line patches with special characters or emoji
- Always verify with `cargo build --release 2>&1 | grep -E "^error|Finished"`
- Always run `cargo test` after any change to src/

---

## 13. What "Complete" Looks Like

SEL Agent is considered **production-ready** when:
1. Protocol Resilience prevents plan parse failures
2. Mutation Enforcement makes weak tests actionable
3. All known FailureKind classifiers are implemented
4. `--dry-run` exists for safe previewing

Current state: **Research-grade agent / early production**.
Comparable to: OpenDevin, SWE-agent in scope and capability.

---

## 14. Quick Start for New Session

Paste this at the start of any new conversation:

```
I'm working on SEL Agent — a Rust autonomous coding agent.
Project: ~/projects/sel-agent-v4/
Current version: v1.2 (complete)
Next: v1.3

Please read SEL_AGENT_REFERENCE.md before we start.
```

Then share this file.

---

*SEL Agent — built in Rust by Chokri Bouzid*
*Binary: ~5MB | No runtime dependencies | Groq API required*
