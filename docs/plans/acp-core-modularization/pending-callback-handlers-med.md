# Story S-ACP-MOD-CALLBACKS: Extract Callback and Local-Tool Handlers

> **Status:** pending | **Difficulty:** med
> **Epic:** [ACP Core Modularization](../pending-acp-core-modularization-hard.md).
> **Depends on:** S-ACP-MOD-BASE. **Blocks:** S-ACP-MOD-TURN.

## Goal

Move inbound notification dispatch, bounded callback scheduling, terminal
management, filesystem callbacks, permission mapping, session MCP attachment,
and stderr diagnostics into focused private modules.

## Acceptance criteria

- [ ] `handlers/` owns `HandlerDeps`, notification projection, and callback
      capacity/cancellation helpers.
- [ ] `handlers/{fs,permission,terminal}.rs` own their corresponding ACP
      request handlers and focused tests.
- [ ] `mcp.rs` owns profile-filtered MCP loading; `diagnostics.rs` owns the
      bounded redacted stderr tail.
- [ ] Per-session callback limit and cancellation semantics are unchanged.
- [ ] Workspace path and terminal CWD containment remain covered by tests.
- [ ] No REST/WS contract changes; run Rust checks and `make test-contract` if
      the changed tests exercise a contract fixture.

## Out of scope

Actor spawn/connection construction, command-loop changes, or permission-policy
changes.
