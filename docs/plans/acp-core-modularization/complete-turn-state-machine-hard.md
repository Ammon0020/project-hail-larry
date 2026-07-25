# Story S-ACP-MOD-TURN: Extract Actor Turn State Machine

> **Status:** complete | **Difficulty:** hard
> **Epic:** [ACP Core Modularization](../pending-acp-core-modularization-hard.md).
> **Depends on:** S-ACP-MOD-ACTOR. **Blocks:** S-ACP-MOD-REGISTRY.

## Goal

Move the actor command protocol and complete prompt/control state machine into
`core/actor/turn.rs` (currently roughly lines 1,291–1,327 and 1,891–2,260).

## Acceptance criteria

- [x] `turn.rs` owns `ActorCommand`, prompt execution, control-RPC dispatch,
      stop-reason mapping, cancellation notification, and close handling.
- [x] Actor handles expose intent-level send methods or a private sender; client
      lifecycle/operations code cannot access a connection or bypass admission.
- [x] Prompt reservation, sticky cancel-before-dequeue, control-command draining,
      cancel/close pre-emption, and nested-prompt rejection preserve ordering.
- [x] Provider, model, and profile commands preserve capability gates and exact
      response/error mapping; prompt lifecycle events remain durably ordered.
- [x] A malicious agent that ignores cancellation is still force-closed after the
      grace period, killing its process group and clearing local permission state.
- [x] Move prompt/cancel/close tests with the turn module; retain end-to-end
      lifecycle coverage and add deterministic regressions for early cancel and
      grace-period force-close if they are not directly covered.
- [x] No public API changes, concurrent prompts, channel-capacity changes, or new
      actor framework.
- [x] Run `cargo fmt -q`, `cargo test -q --all-targets`, and
      `cargo clippy -q --all-targets -- -D warnings`.

## File references

- `src/acp/core.rs:1291-1327`
- `src/acp/core.rs:1891-2260`
- `src/acp/providers.rs`
- `tests/acp_core_lifecycle.rs`
