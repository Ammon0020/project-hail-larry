# Story S-ACP-MOD-ACTOR: Extract Actor Runtime

> **Status:** pending | **Difficulty:** hard
> **Epic:** [ACP Core Modularization](../pending-acp-core-modularization-hard.md).
> **Depends on:** S-ACP-MOD-CALLBACKS. **Blocks:** S-ACP-MOD-TURN.

## Goal

Move agent process creation, ACP SDK connection construction, initialize, and
session new/load resolution into `core/actor/mod.rs`, leaving the still-inline
turn machine behind a narrow temporary call boundary.

## Acceptance criteria

- [ ] Move the current actor configuration/startup and new/load resolution cluster
      (roughly lines 1,330–1,890, excluding the turn loop) as one extraction.
- [ ] `actor::spawn` returns an opaque handle plus typed readiness, registration,
      and terminal-outcome channels; it never receives live/dormant maps or a
      `SessionEntry` and cannot mutate the registry directly.
- [ ] Lifecycle orchestration publishes a session only after readiness and then
      acknowledges registration before the actor consumes commands.
- [ ] Only the actor task calls `connect_with` and owns the long-lived
      `ConnectionTo<Agent>`; no connection is stored in an `Arc`, actor handle,
      callback service, or registry entry.
- [ ] Explicit cwd, piped stdio/stderr, `kill_on_drop`, process-group
      cleanup/reaping, and bounded startup diagnostics are unchanged; agent
      environment policy is not altered by this refactor.
- [ ] Initialize capabilities, config-option ids, initial profile selection, MCP
      attachment, and persisted session load/new fallback preserve behavior.
- [ ] Move process-group, initialize-info, and load/new tests with the actor;
      cover startup failure before publication and unexpected post-startup exit.
- [ ] Run `cargo fmt -q`, `cargo test -q --all-targets`, and
      `cargo clippy -q --all-targets -- -D warnings`.

## File references

- `src/acp/core.rs:1330-1890`
- `src/procutil/mod.rs`
- `tests/spike_acp.rs`

## Out of scope

Changing local stdio transport, adopting a proxy/actor framework, or changing
session history policy.
