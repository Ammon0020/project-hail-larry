# Story S-ACP-MOD-TURN: Extract Single-Owner Actor Turn State Machine

> **Status:** pending | **Difficulty:** hard
> **Epic:** [ACP Core Modularization](../pending-acp-core-modularization-hard.md).
> **Depends on:** S-ACP-MOD-CALLBACKS, S-ACP-MOD-ACTOR.
> **Blocks:** S-ACP-MOD-REGISTRY.

## Goal

Move `ActorCommand`, prompt execution, cancellation, control-RPC dispatch, and
close handling into `turn.rs` while preserving the one-task ACP connection
owner model.

## Acceptance criteria

- [ ] `turn.rs` owns the command enum and all `ConnectionTo<Agent>` request
      and notification use after connection setup.
- [ ] Prompt admission, sticky early cancellation, cancel pre-emption, and
      close pre-emption remain behaviorally identical.
- [ ] Provider, model, and profile control commands retain their existing
      capability gates and response/error mapping.
- [ ] Unexpected actor exit still appends the event, updates state, and clears
      local permissions in the correct order.
- [ ] Existing prompt/cancel/close/rebind tests remain green; add a regression
      test if extraction reveals an ordering ambiguity.
- [ ] Rust format, tests, and clippy pass.

## Out of scope

Concurrent prompts, changing channel capacity, or an external actor framework.
