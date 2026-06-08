# Maintainer Notes

## Source of Truth
- Version source of truth is `Cargo.toml` only.
- CLI and runtime version output must use `env!("CARGO_PKG_VERSION")`.

## Code Change Policy
- Do not patch Rust source through ad-hoc shell/python mutation scripts.
- Do not keep tracked `.bak*` files in the repository.
- Use Git history and tests as the only trusted change record.

## Current Runtime Repair Path
`do_repairing()` currently uses:
- adaptive repair routing
- loop escalation
- smart repair context
- recent edit scoring
- dependency graph caching

## Cleanup Policy
If a temporary migration or patch script is ever needed:
- keep it untracked or move it outside the main source tree
- convert the logic into Rust + tests before merging
- remove the script once the source implementation lands
