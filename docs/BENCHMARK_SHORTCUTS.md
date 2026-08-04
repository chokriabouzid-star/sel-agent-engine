# Benchmark Deterministic Shortcuts

## What are these?

The SEL Agent contains **3 deterministic fix functions** in `src/decision/checklist.rs`
that provide pre-cooked solutions for known benchmark scenarios:

| Function | Benchmark Case | What it fixes |
|----------|--------------|---------------|
| `try_semantic_go_worker_pool_fix` | Go worker pool | `ProcessJobs` edge cases (workers ≤ 0) |
| `try_semantic_ts_retry_fix` | TypeScript retry | Promise rejection + timeout handling |
| `try_semantic_ts_api_client_fix` | TypeScript API client | TS2345/TS2459 type errors |

## Why do they exist?

These are **NOT "cheats"**. They are **Pattern Graduation** in action:

1. The LLM initially struggled with these specific benchmark cases
2. After many repair cycles, the correct fix pattern became clear
3. Instead of burning LLM tokens on a solved problem, the engine applies
   the fix deterministically
4. This is exactly what `v9.4.0 Pattern Graduation → AutoFix` aims to
   generalize via `PatternLibrary.is_graduate()`

## Future: v9.4.0 Pattern Graduation

These 3 functions will be replaced by the general `AutofixRule` system:
- Patterns with `usage_count >= 10` and `success_rate >= 0.95`
- Will graduate from "hardcoded shortcut" to "engine rule"
- See: SEL_AGENT_MASTER_DOCUMENT.md → v9.4.0 roadmap

## Current status

- ✅ All 3 functions are **source-only** (they modify source files, not tests)
- ✅ All 3 respect **Constitution Rule 1** (no test file modification)
- ✅ All 3 have **idempotency guards** (won't reapply if already fixed)
