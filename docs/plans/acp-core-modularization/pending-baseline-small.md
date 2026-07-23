# Story S-ACP-MOD-BASE: Baseline Invariants and Module Seams

> **Status:** pending | **Difficulty:** small
> **Epic:** [ACP Core Modularization](../pending-acp-core-modularization-hard.md).
> **Depends on:** —. **Blocks:** CALLBACKS, ACTOR.

## Goal

Document the existing `core.rs` ownership and ordering invariants, establish
module declarations without behavior changes, and make the later mechanical
moves reviewable.

## Acceptance criteria

- [ ] Record the single-owner `ConnectionTo<Agent>` invariant beside the actor
      boundary and identify the readiness/publication handshake.
- [ ] Add the `core/` module skeleton with private or `pub(super)` visibility;
      keep public re-exports and `ACPClient` behavior unchanged.
- [ ] Identify the tests that protect cancellation, close, rebind, dormant
      restore, notification ordering, terminal containment, and stderr bounds.
- [ ] No production behavior or Cargo dependency changes.
- [ ] `cargo fmt --check -q`, `cargo test -q --all-targets`, and
      `cargo clippy -q --all-targets -- -D warnings` pass.

## Out of scope

Moving implementation code or introducing a `SessionRegistry` abstraction.
