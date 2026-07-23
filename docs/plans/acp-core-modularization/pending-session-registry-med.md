# Story S-ACP-MOD-REGISTRY: Isolate Session Registry and Durable Lifecycle

> **Status:** pending | **Difficulty:** med
> **Epic:** [ACP Core Modularization](../pending-acp-core-modularization-hard.md).
> **Depends on:** S-ACP-MOD-TURN. **Blocks:** S-ACP-MOD-VERIFY.

## Goal

Extract the public `Client` facade's live/dormant metadata and persistence
logic into a narrow registry/lifecycle module without widening the ACP client
interface.

## Acceptance criteria

- [ ] Registry operations centralize live/dormant lookup, state mutation,
      durable metadata load/persist, and one-at-a-time restoration.
- [ ] No lock is held across an await or actor command send.
- [ ] `ACPClient` remains implemented by `Client` and all external signatures
      remain unchanged.
- [ ] Session rename, rebind, model/profile updates, dormant prompt restore,
      and close preserve durable metadata and public status behavior.
- [ ] Tests cover stored-only listing, lazy restore, persisted ACP id, and
      concurrent restore protection.
- [ ] Rust format, tests, and clippy pass.

## Out of scope

Changing the conversation storage format or adopting agent-owned history.
