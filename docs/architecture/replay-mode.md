# Replay Mode Architecture

The Replay system (`src/llm/replay.rs` and `src/llm/record.rs`) allows the SEL Agent to run completely deterministically offline. It is primarily used during benchmark suites and tests to save LLM API costs and execution time.

## Recording Trajectories

When the agent runs in `--record` mode:
1. `RecordProvider` intercepts all calls to the Live LLM provider.
2. It captures the exact input prompt and the exact output string returned by the LLM.
3. These pairs are hashed (using SHA-256 of the prompt) and saved into a local `.sel_hashes` file or a dedicated `fixtures/trajectories/` directory.

## Replaying Trajectories

When the agent runs in `--replay` mode:
1. `ReplayProvider` takes the place of the live LLM.
2. It hashes the incoming prompt and looks for a match in the trajectory files.
3. If a match is found, it instantly returns the recorded LLM response.
4. If a match is not found, it strictly panics or errors out to guarantee that no unrecorded network calls are made during a deterministic test.

## Environment Isolation

When the Agent detects `self.llm.mode() == "replay"`, it also sets `self.executor.replay_mode = true`. This puts the execution environment into a strict offline mode. For example, any commands that attempt to reach out to the internet (like downloading un-cached packages via `npm install` or `pip install`) are either skipped or simulated, guaranteeing complete environment purity.
