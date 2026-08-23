# Story S-ACP-MOD-CALLBACKS: Extract Callback Services

> **Status:** complete | **Difficulty:** hard
> **Epic:** [ACP Core Modularization](../complete-acp-core-modularization-hard.md).
> **Depends on:** —. **Blocks:** S-ACP-MOD-ACTOR.

## Goal

Make the first split substantial: move the complete inbound callback, local-tool,
MCP, event-append, and stderr-diagnostic cluster out of `core.rs` (currently
roughly lines 2,261–2,989, plus its focused tests).

## Acceptance criteria

- [x] Create `events.rs`, `diagnostics.rs`, `mcp.rs`, and
      `handlers/{mod,filesystem,permission,terminal}.rs`; do not create empty
      scaffolding or leave duplicate implementations in `core.rs`.
- [x] `handlers/mod.rs` owns `HandlerDeps`, callback capacity, cancellation, and
      SDK callback dispatch helpers; callback work never blocks SDK dispatch.
- [x] Terminal execution remains permission-gated on the exact argv/cwd/env,
      bounded by terminal count/output limits, and cancelled on session teardown.
- [x] Filesystem and terminal paths retain workspace containment and symlink
      defenses through the existing workspace/path APIs.
- [x] MCP load failures remain additive/non-fatal and profile allowlisting is
      unchanged; stderr remains bounded and obvious credential lines are redacted.
- [x] Move terminal/MCP tests with their modules; add focused regressions for
      callback-capacity rejection, callback cancellation, path escape rejection,
      and bounded/redacted diagnostics where coverage is currently missing.
- [x] `core.rs` loses the whole responsibility cluster, not merely imports or
      wrappers. No public exports, REST/WS behavior, or permission policy change.
- [x] Run `cargo fmt -q`, `cargo test -q --all-targets`, and
      `cargo clippy -q --all-targets -- -D warnings`.

## File references

- `src/acp/core.rs`
- `src/acp/core/{events,diagnostics,mcp}.rs`
- `src/acp/core/handlers/{mod,filesystem,permission,terminal}.rs`
- `src/acp/stream.rs`
- `src/mcp/mod.rs`

## Out of scope

Agent process startup, the actor command/turn loop, registry state, or new tool
capabilities.
