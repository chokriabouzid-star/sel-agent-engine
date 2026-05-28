# Mutation Testing System

The Mutation System (`src/state_handlers.rs` and `src/executor.rs`) is an advanced mechanism to grade the quality of the LLM's test coverage and logic.

## Mutation Generation

Once the LLM successfully writes passing code, the Agent attempts to intentionally "break" the source code to see if the tests still pass. This ensures the tests are actually asserting behavior and aren't simply "hollow" passing tests.
1. The `executor.mutation_check(src)` reads the modified source file.
2. It locates critical logical operators, boolean values, or assignments (e.g., changing `==` to `!=`, `+` to `-`, `true` to `false`).
3. It creates a mutated version of the source file.

## Mutation Verification

1. The agent reruns the test suite against the mutated code.
2. **Killed**: If the tests fail (as they should, because the code is broken), the mutation is marked as `killed`.
3. **Survived**: If the tests pass despite the broken code, the mutation `survived`. This indicates that the LLM wrote weak tests or that the code segment is unreachable/dead code.

## Scoring and Feedback

The system tracks `mutations_total` and `mutations_killed` in the `ExecutionContext`.
The agent computes a `mutation_score` (Killed / Total). A low mutation score can be fed back into the LLM as a subsequent objective, instructing it to tighten its assertions. This score is also logged to the `SEL_OBSERVATORY` for benchmark grading.
