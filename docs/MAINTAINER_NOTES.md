# Maintainer Notes

## Source of Truth
- Version source of truth is `Cargo.toml` only.
- CLI and runtime version output must use `env!("CARGO_PKG_VERSION")`.

## Code Change Policy
- Do not patch Rust source through ad-hoc shell/python mutation scripts.
- Do not keep tracked `.bak*` files in the repository.
- Use Git history and tests as the only trusted change record.

## Runtime Repair Path
`do_repairing()` currently uses:
- adaptive repair routing
- loop escalation
- smart repair context
- recent edit scoring
- dependency graph caching

## Architecture Risk Zones

### `src/constitution.rs`
- Changes here affect the hard rules of the agent.
- Risk: opening a path for the agent to violate the test contract.

### `src/state_handlers.rs`
- Core state machine behavior lives here.
- Risk: a bad transition may silently move `Repairing` to `Done` or skip failure handling.

### `src/agent.rs`
- Owns the main run loop and final outcome decisions.
- Risk: incorrect orchestration can invalidate the overall control flow.

### `src/pattern_library.rs`
- Stores learned repair signals.
- Risk: a low-quality pattern becoming "strong" can poison future repairs.

### `fixtures/trajectories/`
- Replay determinism depends on these artifacts.
- Risk: manual edits can silently break reproducibility.

## Cleanup Policy
If a temporary migration or patch script is ever needed:
- keep it untracked or move it outside the main source tree
- convert the logic into Rust + tests before merging
- remove the script once the source implementation lands
