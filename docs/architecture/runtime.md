# SEL Agent Runtime Architecture

The runtime architecture of the SEL Agent is centered around a robust State Machine engine (`src/agent.rs` and `src/state_handlers.rs`).

## State Machine (`AgentState`)

The agent operates in a strict state loop transitioning through:
1. **Planning**: The agent communicates with the LLM to process the user's `goal` and generate a sequence of commands (`Cmd`).
2. **Executing**: The `SafeExecutor` runs the commands. Before execution, a snapshot of the workspace is taken. If execution succeeds, the state transitions to `Done` (or `Repairing` if tests fail). If a critical failure occurs, the state transitions to `Failed` and the snapshot is rolled back.
3. **Repairing**: If tests or compilation fail, the agent builds contextual prompts (managed by the Repair Engine) to fix the errors. It tracks attempt counts and loop detection fingerprints.
4. **WaitingForUserInput**: If the agent exhausts its `max_repairs` limits, it falls back to EXPLAIN MODE, where the execution pauses, waiting for a human developer to provide a hint. (This is skipped automatically in Bench Mode).
5. **Done / Failed**: Final terminal states.

## Workspace Snapshots (`src/snapshot.rs`)

The Agent uses Git for atomic, deterministic rollbacks of the workspace.
- Before `Agent::run` delegates to execution, it creates an `Initial Commit` baseline.
- `Snapshot::take()` pushes untracked and tracked changes to a git stash.
- If a step fails, `Snapshot::rollback()` does a `git reset --hard` and `git clean -fd` to purge LLM hallucinations/corruptions.
- If a step succeeds, `Snapshot::commit()` drops the stash.

## Preflight Checks & Scaffold Engine

Before entering the state machine, the agent runs the `scaffold_engine` which parses the `goal` and prepares the workspace (e.g., creating `venv` for Python, `package.json` for TypeScript). 
A preflight scan checks if the workspace has tests. If no tests are found (and it's not a "creation task"), the agent warns the user since it cannot verify its own fixes automatically.

## Observability

The Agent asynchronously reports execution events, repairs, and mutation scores to the `SEL_OBSERVATORY` via HTTP POST requests for observability and telemetry.
