# Story S-ACP-MOD-FACADE: Finish Client Facade and Lifecycle Split

> **Status:** pending | **Difficulty:** hard
> **Epic:** [ACP Core Modularization](../pending-acp-core-modularization-hard.md).
> **Depends on:** S-ACP-MOD-REGISTRY.

## Goal

Remove the remaining implementation from `core.rs` by separating the stable
public facade from lifecycle orchestration and live session operations.

## Acceptance criteria

- [ ] `client.rs` owns `ClientDeps`, `Client`, construction/profile accessors, and
      the single `ACPClient` implementation; trait methods are thin delegation.
- [ ] `lifecycle.rs` owns create, lazy restore, rebind, cancel watchdog, close,
      actor readiness/publication, actor-outcome monitoring, and cleanup ordering.
- [ ] `operations.rs` owns prompt preparation/admission and provider, model, and
      profile operations without reaching into registry locks or actor internals.
- [ ] Unexpected actor exit appends the stable exit event, marks the matching
      actor generation failed, and clears permission state. Add the generation
      guard required by channel-based outcomes and test that a stale replaced
      actor outcome cannot fail the replacement session.
- [ ] Rebind preserves local session id/history, clears an agent-specific ACP id,
      refreshes capabilities/config ids, and exposes the replacement only after
      its readiness handshake.
- [ ] Move focused unit tests to their owner modules and keep rebind, dormant
      restore, prompt/cancel/close, profile, and concurrent-session behavior in
      `tests/acp_core_lifecycle.rs` or equivalent cross-module tests.
- [ ] Rewrite `set_session_profile_leaves_local_state_on_rpc_failure` to inject a
      failing actor handle through production internal APIs and assert through
      client/registry accessors; do not expose `SessionEntry` fields or add a
      test-only public API.
- [ ] Add coverage that concurrent restore starts one actor and that the existing
      `MAX_SESSIONS` limit rejects excess live sessions without spawning a child.
- [ ] `src/acp/core.rs` is at most 150 lines of module declarations, documentation,
      and re-exports; no destination module simply contains the old megafile.
- [ ] `src/acp/mod.rs` continues exporting `Client`, `ClientDeps`, `SessionState`,
      and `STDERR_TAIL_BYTES` at their existing paths.
- [ ] Run `cargo fmt -q`, `cargo test -q --all-targets`, and
      `cargo clippy -q --all-targets -- -D warnings`.

## File references

- `src/acp/core.rs:88-1290`
- `src/acp/mod.rs`
- `tests/acp_core_lifecycle.rs`

## Out of scope

Changing `ACPClient`, REST/WS contracts, profile semantics, storage format, or
adding new session capabilities.
