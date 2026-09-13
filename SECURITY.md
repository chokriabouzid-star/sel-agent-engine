# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 9.3.x   | ✅ Yes    |
| < 9.3   | ❌ No     |

## Known Limitations (by design)

- The agent requires write access to the workspace directory.
- In `run` mode (non-bench), the agent modifies git state in the workspace.
- Test files may be modified by the internal repair engine (autofix/checklist).
- The agent creates `.git`, `.gitignore`, and commits inside the workspace.

## Threat Model

The agent is designed for use in isolated environments (containers or sandboxes).
Do NOT run against workspaces containing secrets or uncommitted work without
a full git backup.

## Fixed in v9.3.5

| ID    | Description |
|-------|-------------|
| C-01  | Snapshot rollback could destroy unsaved files when stash failed |
| C-02  | Autofix wrote to test files without workspace boundary check |
| C-03  | safe_path did not resolve symlinks — escape via symlink was possible |
| H-01  | mkdir accepted `..` paths — directory escape outside workspace |
| H-04  | npm install executed lifecycle scripts (preinstall/postinstall) |
| M-02  | delete_file could delete protected test files |
| M-13  | ReplayProvider ignored request content entirely |
| M-14  | Stale constitution hash produced warning only, not rejection |

## Reporting Vulnerabilities

Open an issue tagged `security` on the repository.
For sensitive issues, contact the maintainer directly before public disclosure.
