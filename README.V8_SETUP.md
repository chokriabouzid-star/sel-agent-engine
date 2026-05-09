SEL-Agent v8.0 readiness scaffolding - Quick Guide
-------------------------------------------------

Files created:
 - fixtures/multi_file_bench/    (5 example multi-file scenarios)
 - scripts/canonicalize_prompt.py
 - scripts/generate_trajectory_index.py
 - scripts/stage_dry_run.sh
 - scripts/apply_staging.sh
 - .git/hooks/pre-commit
 - fixtures/trajectories/example_task/001.json

Suggested next steps:
 1. Inspect fixtures: `ls -R fixtures/multi_file_bench`
 2. Run dry-run for a staging directory:
    - prepare .sel-staging with desired edits
    - ./scripts/stage_dry_run.sh
 3. Generate trajectories index (after you have real trajectories):
    - python3 scripts/generate_trajectory_index.py
 4. Canonicalize a prompt:
    - cat some_file.txt | python3 scripts/canonicalize_prompt.py

Pre-commit:
 - The pre-commit will run `cargo run -- bench --suite all --replay` if core files changed.
 - To bypass locally for testing: `git commit -n` (use with caution).

Notes:
 - This scaffold does not change your source; it creates helper files.
 - Some tests/tools require dependencies (cargo, rust, python, pytest, node, go).
