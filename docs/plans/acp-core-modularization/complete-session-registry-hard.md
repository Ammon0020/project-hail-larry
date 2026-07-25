# Story S-ACP-MOD-REGISTRY: Extract Session Registry

> **Status:** complete | **Difficulty:** hard
> **Epic:** [ACP Core Modularization](../pending-acp-core-modularization-hard.md).
> **Depends on:** S-ACP-MOD-TURN. **Blocks:** S-ACP-MOD-FACADE.

## Goal

Move live/dormant metadata, state transitions, actor-handle storage, and durable
metadata snapshots into `core/registry.rs`; make it impossible for callers to
hold registry locks across async work.

## Acceptance criteria

- [x] `SessionRegistry` owns `SessionState`, `SessionEntry`, live/dormant maps,
      merged list/get behavior, state transitions, and conversation persistence.
- [x] A live entry stores an opaque actor handle and negotiated metadata, never a
      connection or raw command receiver; the actor module has no registry import.
- [x] Registry methods return owned snapshots/handles and expose no lock guards.
      Actor sends, workspace lookups, event appends, and other awaits occur after
      registry transactions end.
- [x] Expose synchronous publish/remove primitives so lifecycle can commit a
      dormant-to-live promotion after readiness without briefly losing the
      dormant record; actor startup and restore serialization stay in lifecycle.
- [x] Persistence snapshots merge live and dormant records without duplicate ids,
      preserve `acpSessionId`, and normalize stale restart status as today.
- [x] Move stored-only listing, merged snapshots, rename round-trip, state
      transition, and persisted-id tests with the registry; lifecycle tests remain
      with the facade story.
- [x] Existing create, rename, rebind, model/profile update, close, and list
      behavior remains unchanged through the new registry API.
- [x] Run `cargo fmt -q`, `cargo test -q --all-targets`, and
      `cargo clippy -q --all-targets -- -D warnings`.

## File references

- `src/acp/core.rs:101-157`
- `src/acp/core.rs:296-408` and `src/acp/core.rs:521-617`
- `src/acp/core.rs:655-715` and `src/acp/core.rs:788-809`
- `src/acp/core.rs:419-519` remains lifecycle orchestration
- `src/acp/core.rs:1271-1289`
- `src/acp/store.rs`

## Out of scope

Conversation-format changes, agent-owned-history policy, or replacing locks with
a database-backed runtime registry.
