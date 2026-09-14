# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 9.3.x   | Yes       |
| < 9.3   | No        |

## Fixed in v9.3.5 (P0 Safety — confirmed with acceptance tests)

| ID   | Description | Acceptance Test |
|------|-------------|----------------|
| C-01 | Snapshot rollback destroyed files when stash failed (index.lock) | 3 tests |
| C-02 | Autofix modified protected test files inside workspace | checklist guard |
| C-03 | safe_path did not resolve symlinks — escape via symlink was possible | 2 tests |
| H-01 | mkdir accepted .. paths — directory escape outside workspace | 2 tests |
| H-04 | npm install executed lifecycle scripts (preinstall/postinstall) | --ignore-scripts |
| H-05 | Agent committed scaffold_baseline on existing git repos | has_head check |
| H-08 | Scaffold replaced devDependencies and deleted jest.config.js | merge strategy |
| M-02 | delete_file could delete protected test files | 3 tests |
| M-13 | ReplayProvider ignored system prompt changes on all steps | 2 tests |
| M-14 | Stale constitution hash produced warning only, not rejection | 3 tests |

## CVE Fixed

| ID | Crate | Vulnerable | Fixed |
|----|-------|-----------|-------|
| RUSTSEC-2026-0258 | h2 | 0.4.14 | 0.4.19 |

## Replay Fixture Policy

Fixtures must match the current constitution hash and system prompt prefix.
cargo test will fail with REPLAY_STALE if fixtures are outdated.

To migrate stale fixtures:

    SEL_ALLOW_STALE_REPLAY=1 cargo test
    then re-record: ./target/debug/sel-agent --record ...

## Threat Model

The agent is designed for use in isolated environments (containers or sandboxes).
Do NOT run against workspaces containing secrets or uncommitted work without
a full git backup.

Known limitations:
- The agent requires write access to the workspace directory
- In run mode, the agent modifies git state in the workspace
- Test files may be modified by autofix/checklist within authorized scope only

## Warnings (non-critical, no fix in 9.3.x)

| ID | Crate | Issue |
|----|-------|-------|
| RUSTSEC-2026-0253 | lru 0.12.5 | unsound: use-after-free in LruCache::pop() |
| RUSTSEC-2026-0002 | lru 0.12.5 | unsound: IterMut violates Stacked Borrows |
| RUSTSEC-2024-0436 | paste 1.0.15 | unmaintained |
| — | chacha20 0.10.1 | yanked |

## Reporting Vulnerabilities

Open an issue tagged security on the repository.
For sensitive issues, contact the maintainer directly before public disclosure.
