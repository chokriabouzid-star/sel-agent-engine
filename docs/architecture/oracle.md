# Workspace Oracle

The `WorkspaceOracle` (`src/workspace_oracle.rs`) is the agent's contextual awareness module. It inspects the local directory and makes strict, deterministic decisions about the rules of the project.

## Language and Framework Detection

By inspecting the root directory, it classifies the workspace into one of several `ProjectType` enums:
- `Rust` (`Cargo.toml`)
- `Go` (`go.mod`)
- `Node` (`package.json`)
- `Python` (`setup.py`, `pyproject.toml`, `requirements.txt`, `venv`, or `.py` files)

## Command Resolution (`resolve_test_command`)

Since the LLM might hallucinate arbitrary or generic test commands, the Oracle provides the ground-truth command string to execute tests based on the detected language:
- **Python**: Uses `venv/bin/pytest` if available, otherwise `pytest`, passing `-v --tb=short`.
- **Node**: Differentiates between `npm test` and `npx jest --runInBand`.
- **Rust**: Uses `cargo test -- --nocapture`.
- **Go**: Uses `go test ./... -v`.

## Constraint Validation (`is_ext_allowed` & `validate_plan_cmd`)

The Oracle implements a strict `LANGUAGE LOCK`. 
If the workspace is detected as a Python project, it blocks the LLM from executing commands like `cargo test` or `npm install`. It also blocks the LLM from attempting to edit or create files with mismatched extensions (e.g., blocking `.rs` files in a Node project). This prevents hallucinations from destroying the workspace structure.
