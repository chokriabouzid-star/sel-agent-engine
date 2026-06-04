# SEL Agent — Project Handoff

## Current verified state
- version: v8.5.2
- cargo clippy -D warnings: pass
- bench --suite all --replay: 36/36
- bench-swe --lang all --replay: 30/30
- bench-sel-v11 --replay: 18/18
- bench-real-world --replay: 14/14
- smoke --replay: 12/12

## Last important root-cause fix
Python replay environment mismatch in `src/executor/runner.rs`.

Replay now restores cached Python venv when a recorded trajectory expects `venv/bin/pytest`, preventing false replay failures and false `TRAJECTORY_INCOMPLETE`.

## Important current architecture points
- deterministic Go autofix chain is active
- semantic repair exists for Go worker pool, TS retry, TS API client, Python stdlib pip misuse
- pre-existing tests are protected via workspace snapshot
- replay mode is now materially more faithful for Python pytest trajectories

## Next priorities
1. v8.6.0 unified reports + observatory
2. v8.6.1 stability layer + trajectory manifest
3. v8.8.0 dependency graph
