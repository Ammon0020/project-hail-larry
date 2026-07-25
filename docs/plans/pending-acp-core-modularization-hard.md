# Epic: ACP Core Modularization

> **Status:** pending. **Owner:** —. **Created:** 2026-07-22.
> **Updated:** 2026-07-24 (CALLBACKS complete). **Difficulty:** hard.
> **Related:** `src/acp/core.rs`, `src/acp/{providers,context,store}.rs`,
> `tests/acp_core_lifecycle.rs`. Stories: `docs/plans/acp-core-modularization/`.

## Goal

Replace the 3,855-line `src/acp/core.rs` megafile with cohesive private modules
while preserving the public
`acp::{Client, ClientDeps, SessionState, STDERR_TAIL_BYTES}` surface, ACP
behavior, and process/path/permission security guarantees. The first story
must remove a complete, large responsibility cluster; no skeleton-only split is
part of this epic.

## Current anatomy

- Lines 1–1,290: public client, live/dormant session state, lifecycle operations,
  persistence, prompt admission, provider/profile/model controls.
- Lines 1,291–2,260: actor command protocol, process/SDK startup, session
  new/load, prompt/control turn machine, cancellation.
- Lines 2,261–2,989: inbound callbacks, terminals, MCP attachment, filesystem,
  permission mapping, path containment, and stderr diagnostics.
- Lines 2,990–3,855: unit and cross-module lifecycle tests.

Line ranges are discovery aids, not instructions to move code blindly.

## Architecture decisions

- One task per session exclusively owns the SDK connection. A connection may be
  passed within `core::actor`, but never stored in the registry, shared through
  `Arc`, or exposed outside the actor subtree.
- `actor` reports typed startup and terminal outcomes to lifecycle orchestration;
  it does not receive the live-session map or mutate `SessionEntry` directly.
- `SessionRegistry` owns live/dormant metadata and synchronous state transitions.
  It stores an opaque actor handle, never a connection, and exposes no lock guard.
- Lifecycle code owns create/restore/rebind/close ordering. Registry publication
  occurs only after actor readiness; the actor starts command consumption only
  after publication acknowledgement.
- Event append/publish remains one ordered async boundary. Handler callbacks stay
  bounded per session, cancellation-aware, and independent of registry locks.
- New modules are private or `pub(super)`; `src/acp/mod.rs` keeps the current
  re-exports. This epic does not change REST/WS payloads or storage formats.
- Focused tests move with implementations. Cross-module lifecycle behavior stays
  in integration coverage rather than gaining test-only public APIs.

## Dependency direction

```text
client -> lifecycle + operations -> registry + actor
actor -> actor::turn + handlers + mcp + diagnostics + events
actor::turn -> providers + events
handlers -> events
registry -> opaque actor handle
actor -X-> registry
```

The final dependency is intentionally one-way: actor failure is an outcome
consumed by lifecycle orchestration, not a callback into registry internals.

## Destination

```text
src/acp/core.rs                       private module declarations + re-exports
src/acp/core/client.rs                Client/ClientDeps and thin ACPClient facade
src/acp/core/lifecycle.rs             create, restore, rebind, cancel, close
src/acp/core/operations.rs            prompt, provider, model, profile operations
src/acp/core/registry.rs              live/dormant metadata and persistence
src/acp/core/events.rs                ordered typed-event append/publish
src/acp/core/diagnostics.rs           bounded/redacted agent stderr
src/acp/core/mcp.rs                   profile-filtered session MCP attachment
src/acp/core/handlers/mod.rs          callback dependencies/capacity/cancellation
src/acp/core/handlers/filesystem.rs   workspace file callbacks and path mapping
src/acp/core/handlers/permission.rs   ACP permission projection
src/acp/core/handlers/terminal.rs     terminal lifecycle and retained output
src/acp/core/actor/mod.rs             process, SDK connection, init/new/load
src/acp/core/actor/turn.rs            commands, prompt/control loop, cancellation
```

## Story index

| ID | Story | Difficulty | Depends on | Status |
|----|-------|------------|------------|--------|
| S-ACP-MOD-CALLBACKS | [Extract callback services](acp-core-modularization/complete-callback-services-hard.md) | hard | — | complete |
| S-ACP-MOD-ACTOR | [Extract actor runtime](acp-core-modularization/pending-actor-runtime-hard.md) | hard | CALLBACKS | pending |
| S-ACP-MOD-TURN | [Extract actor turn state machine](acp-core-modularization/pending-turn-state-machine-hard.md) | hard | ACTOR | pending |
| S-ACP-MOD-REGISTRY | [Extract session registry](acp-core-modularization/pending-session-registry-hard.md) | hard | TURN | pending |
| S-ACP-MOD-FACADE | [Finish client facade and lifecycle split](acp-core-modularization/pending-client-facade-hard.md) | hard | REGISTRY | pending |

**Sequence:** CALLBACKS → ACTOR → TURN → REGISTRY → FACADE. Each story must
leave the branch green and remove a complete implementation cluster from
`core.rs`; none is a module-skeleton or research-only story.

## Scope

**In scope:** internal module moves, narrow internal interfaces, removal of the
actor-to-registry back-reference, test relocation, and missing regression tests
for existing security/concurrency invariants.

**Out of scope:** external API behavior, conversation format changes, concurrent
prompts, permission-policy changes, MCP-over-ACP brokerage, HTTP ACP transport,
proxy frameworks, or replacing `async-process`.

## Completion criteria

- `core.rs` is at most 150 lines and contains no actor, callback, registry, or
  lifecycle implementation.
- No replacement module becomes another megafile; each destination above owns
  only its named concern.
- Actor connection ownership, ready/publish ordering, ordered events, live
  session/callback bounds, path containment, process-group cleanup, and cancel
  force-close have direct regression coverage.
- `cargo fmt -q`, `cargo test -q --all-targets`, and
  `cargo clippy -q --all-targets -- -D warnings` pass.
