# Repair Engine

The Repair Engine (`src/repair_strategy.rs`) is responsible for recovering the agent when tests or commands fail. It implements an escalating prompt strategy and sophisticated loop detection.

## Escalating Prompts

The engine does not send the same error message repeatedly. Instead, it increments the prompt severity based on `ctx.repair_attempts`:
- **Attempt 1**: Standard error report and request for a fix.
- **Subsequent Attempts**: It requests the LLM to rewrite the entire function or file from scratch rather than attempting incremental surgical edits, since surgical edits often fail to resolve structural bugs.

## Error Loop Detection

To prevent the LLM from getting stuck in an infinite loop of writing the same bug and getting the same error:
- The engine hashes the modified source files into `fingerprints` (u64).
- It compares the current fingerprint to historical fingerprints (`ctx.repair_fingerprints`).
- If a duplicate state is detected (the LLM reverted to an exact previous state), the prompt is injected with a `[LOOP DETECTED]` warning, instructing the LLM to take a drastically different approach.
- It differentiates between simple loops and consecutive loops.

## Context Extraction

The repair engine intelligently distinguishes between source files and test files (e.g., checking if a file is `_test.go`, `.spec.ts`, or uses `pytest`).
When a test fails, it provides the LLM with the error logs, but restricts the LLM from making changes to the test files if the goal is to fix the application code.

## Unicode Safety

Test runners (like Jest) often output multi-byte unicode characters (e.g., `✕`). The repair engine safely truncates long error messages using `safe_prefix`, which respects character boundaries (`is_char_boundary`), preventing runtime panics when slicing `String` bytes.
