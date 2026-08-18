# Contributing to SEL Agent

Thank you for your interest in contributing. SEL Agent enforces strict quality
gates — please read this document before opening a PR.

---

## Environment Requirements

| Tool | Version |
|------|---------|
| Rust | stable (see `rust-toolchain` if present) |
| Cargo | bundled with Rust |
| Go | 1.22+ (for Go bench tasks) |
| Node / npm | 18+ (for TypeScript bench tasks) |
| Python | 3.10+ (for Python bench tasks) |

Required environment variables (at minimum):
```bash
export GROQ_API_KEY=<your-key>
```

---

## Quality Gate (mandatory before every PR)

All of the following must pass — no exceptions:

```bash
# 1. Format
cargo fmt --all

# 2. Lint
cargo clippy --all-targets --all-features -- -D warnings

# 3. Unit + integration tests
cargo test

# 4. Evidence tests
cargo test evidence_

# 5. Core regression gate
bash scripts/regression_gate.sh core
```

For any PR that touches the execution pipeline, repair routing, or constitution:

```bash
bash scripts/regression_gate.sh full
```

---

## Commit Format

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <short description>

[optional body]
```

| Type | When to use |
|------|-------------|
| `feat` | New capability |
| `fix` | Bug fix |
| `refactor` | Code restructure without behaviour change |
| `docs` | Documentation only |
| `test` | Tests only |
| `chore` | Build, CI, tooling |
| `perf` | Performance improvement |
| `release` | Version bump + changelog update |

**Examples:**
```
feat(autofix): support missing comma in _test.go files
fix(constitution): block rm -rf . and rm -rf ~
docs(reference): update REFERENCE.md to v9.3.5
refactor(decision): split decision.rs into facade + 5 submodules
```

---

## The Non-Negotiable Rules

These mirror the agent's own constitution — contributors must follow them too:

1. **Never modify existing test files.** If a test is wrong, discuss it first.
2. **No empty writes, no binary content in text files.**
3. **No `.bak` files or ad-hoc patch scripts in the repo.** Use Git history.
4. **No changes to `go.mod` without explicit justification.**
5. **Every new capability must have an impact eval** (`evals/feature_impact/`).
6. **Refactors go in a separate milestone/branch** from features.

---

## PR Checklist

Before opening a pull request:

- [ ] `cargo fmt --all` — no diff
- [ ] `cargo clippy -- -D warnings` — 0 warnings
- [ ] `cargo test` — all pass
- [ ] `cargo test evidence_` — all pass
- [ ] `bash scripts/regression_gate.sh core` — PASS
- [ ] CHANGELOG.md updated under the correct `[Unreleased]` or version section
- [ ] No untracked `.bak` or temporary files committed
- [ ] PR description explains *why*, not just *what*

---

## Versioning

Version source of truth is **`Cargo.toml` only**.
CLI and runtime must use `env!("CARGO_PKG_VERSION")` — never hardcoded strings.

Version format: `MAJOR.MINOR.PATCH` (SemVer).

---

## Questions

Open a GitHub Issue. No external chat channels — everything is tracked in Git.
