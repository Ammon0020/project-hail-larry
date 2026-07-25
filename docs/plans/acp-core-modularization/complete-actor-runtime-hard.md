# Story S-ACP-MOD-ACTOR: Extract Actor Runtime

> **Status:** complete | **Difficulty:** hard
> **Epic:** [ACP Core Modularization](../complete-acp-core-modularization-hard.md).
> **Depends on:** S-ACP-MOD-CALLBACKS. **Blocks:** S-ACP-MOD-TURN.

## Goal

Move agent process creation, ACP SDK connection construction, initialize, and
session new/load resolution into `core/actor/mod.rs`, leaving the still-inline
turn machine behind a narrow temporary call boundary.

## Acceptance criteria

- [x] Move the current actor configuration/startup and new/load resolution cluster
      (roughly lines 1,330–1,890, excluding the turn loop) as one extraction.
- [x] `actor::spawn` returns an opaque handle plus typed readiness, registration,
      and terminal-outcome channels; it never receives live/dormant maps or a
      `SessionEntry` and cannot mutate the registry directly.
- [x] Lifecycle orchestration publishes a session only after readiness and then
      acknowledges registration before the actor consumes commands.
- [x] Only the actor task calls `connect_with` and owns the long-lived
      `ConnectionTo<Agent>`; no connection is stored in an `Arc`, actor handle,
      callback service, or registry entry.
- [x] Explicit cwd, piped stdio/stderr, `kill_on_drop`, process-group
      cleanup/reaping, and bounded startup diagnostics are unchanged; agent
      environment policy is not altered by this refactor.
- [x] Initialize capabilities, config-option ids, initial profile selection, MCP
      attachment, and persisted session load/new fallback preserve behavior.
- [x] Move process-group, initialize-info, and load/new tests with the actor;
      cover startup failure before publication and unexpected post-startup exit.
- [x] Run `cargo fmt -q`, `cargo test -q --all-targets`, and
      `cargo clippy -q --all-targets -- -D warnings`.

## Implementation notes

- Actor startup, SDK connection construction, initialize, session new/load
  resolution, and stderr-bound startup diagnostics moved to
  `src/acp/core/actor/mod.rs` (~850 lines).
- The `sessions` field and `fail_session` back-reference were removed; the actor
  reports failure through a `TerminalOutcome` channel consumed by a lifecycle
  terminal watcher that owns registry mutation and `AgentExited` publication.
- `actor::Handle` is an opaque, cloneable handle with a monotonic id for
  staleness checks after rebind replaces an actor.
- The turn machine (`actor_loop`, `await_prompt`, `handle_non_prompt_command`,
  etc.) remains in `core.rs` for S-ACP-MOD-TURN; `actor_loop` is called via
  `super::actor_loop` from the actor module.
- `mockagent` gained `MOCKAGENT_EXIT_AFTER_INIT` to exercise the post-startup
  exit path.
- `src/acp/core.rs` reduced from ~2,957 to ~2,233 lines.

## File references

- `src/acp/core/actor/mod.rs`
- `src/acp/core.rs`
- `src/procutil/mod.rs`
- `cmd/mockagent/main.go`
- `tests/spike_acp.rs`

## Out of scope

Changing local stdio transport, adopting a proxy/actor framework, or changing
session history policy.
