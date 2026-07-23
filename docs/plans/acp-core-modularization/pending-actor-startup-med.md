# Story S-ACP-MOD-ACTOR: Extract Agent Process and SDK Connection Setup

> **Status:** pending | **Difficulty:** med
> **Epic:** [ACP Core Modularization](../pending-acp-core-modularization-hard.md).
> **Depends on:** S-ACP-MOD-BASE. **Blocks:** S-ACP-MOD-TURN.

## Goal

Move agent child-process startup, process-group cleanup, ACP SDK handler
builder, initialize, and session new/load resolution into `actor.rs`.

## Acceptance criteria

- [ ] The process still has explicit cwd, piped stdio/stderr, `kill_on_drop`,
      and process-group cleanup/reaping.
- [ ] The only long-lived `ConnectionTo<Agent>` value stays inside the actor
      flow; setup exposes no shared connection handle.
- [ ] Initialize capabilities, provider/profile config ids, and persisted ACP
      session-id resolution preserve current behavior.
- [ ] Agent stderr remains bounded and does not disclose obvious credentials in
      startup errors.
- [ ] Startup failure and successful ready/registered handshakes have coverage.
- [ ] Rust format, tests, and clippy pass.

## Out of scope

Changing transport from local stdio to HTTP/WebSocket or adopting a proxy.
