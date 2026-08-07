# Wire MCP-over-ACP broker via agent-client-protocol-polyfill

> **Status:** complete | **Difficulty:** hard | **Urgency:** high
> **Epic:** [profiles-over-acp](../complete-profiles-over-acp-hard.md) (loose follow-up)
> **Completed:** commit `f0bddd9`. Remaining scope moved to
> [`pending-mcp-over-acp-handlers-hard-high.md`](pending-mcp-over-acp-handlers-hard-high.md).

## Goal

Wire the MCP-over-ACP broker using the SDK's `agent-client-protocol-polyfill`
crate, replacing the unbuilt broker design in `docs/plans/acp-spec-compliance.md`
and unblocking the "MCP-over-ACP — broker not wired" gap in `docs/STATUS.md`.

## Background

The project currently uses **inline MCP**: `load_session_mcp_servers`
(`src/acp/core.rs`) reads `mcp.json`, filters by profile policy, and attaches
`McpServer::Stdio`/`Http`/`Sse` to `session/new`. This works but means MCP
servers are fixed at session creation — they can't be added/removed live.

The `unstable_mcp_over_acp` feature flag is enabled in `Cargo.toml` but
`grep` for `mcp/message|UnstableConnectMcp|UnstableDisconnectMcp` in `src/acp/`
returns **zero** matches — the broker is completely unwired.

## What the polyfill crate offers

`agent-client-protocol-polyfill` (published 2.0.0, aligned with SDK 2.0)
provides `McpOverAcpPolyfill` — a proxy that:

1. **Adapts native MCP-over-ACP to HTTP** for agents that don't support
   `mcpCapabilities.acp`. Intercepts `NewSessionRequest`, rewrites
   `McpServer::Http` entries with `acp:` URLs into localhost bridges.
2. **Sets `mcpCapabilities.acp = true`** on init so the client knows MCP-over-ACP
   is available.
3. **Bridge modes**: `BridgeMode::Http` (default) or `BridgeMode::Stdio` (with
   `conductor_command` — note: stdio mode has zero test coverage upstream).

This directly addresses the project's gap: agents that support MCP-over-ACP
get native `mcp/connect` / `mcp/message` / `mcp/disconnect`; agents that don't
are transparently bridged via HTTP.

## Approach

1. **Add the dependency**:
   ```toml
   agent-client-protocol-polyfill = "2.0.0"
   ```

2. **Insert the polyfill in the client→agent chain**: When creating a session,
   if the agent doesn't advertise `mcpCapabilities.acp`, route through
   `McpOverAcpPolyfill` so inline MCP servers are bridged. This is a
   client-side proxy — no agent changes needed.

3. **Wire the MCP-over-ACP relay methods**: With the polyfill in place, wire
   `mcp/connect`, `mcp/message`, `mcp/disconnect` in the actor loop
   (`src/acp/core.rs`). The `unstable_mcp_over_acp` feature generates the
   schema-native types (`McpServer::Acp`, `mcp/connect`, `mcp/message`,
   request/response `mcp/disconnect`) — they just need handlers.

4. **Update the profile MCP server policy**: The active
   `active-profile-mcp-transition-hard-high.md` story selects complete MCP
   servers at session setup. With the polyfill, live add/remove becomes
   possible for agents that support MCP-over-ACP — the transition dialog's
   "instructions only" option could optionally reconnect MCP servers instead
   of requiring a new session.

5. **Contract tests**: Add coverage for the polyfill bridge path and the native
   MCP-over-ACP path.

## Risks

- **Architectural change**: This changes how MCP servers are attached to
  sessions. The current inline-MCP path works; adding a proxy layer increases
  complexity. Evaluate whether the benefit (live MCP add/remove) justifies the
  cost.
- **Stdio bridge mode untested upstream**: Use `BridgeMode::Http` (default).
  Avoid `BridgeMode::Stdio` until upstream test coverage improves.
- **`unstable_mcp_over_acp` is unstable**: The API may change between SDK
  versions. Pin to 2.0.0 and monitor for breaking changes.
- **Interaction with profile transition story**: The
  `active-profile-mcp-transition-hard-high.md` story assumes MCP servers are
  fixed at session creation. If the polyfill enables live changes, the
  transition dialog design may need revision. Coordinate the two stories.

## Dependencies

- Requires `agent-client-protocol` 2.0.0 (done — upgraded 2026-08-05).
- Coordinate with `active-profile-mcp-transition-hard-high.md` — the polyfill
  may change the transition dialog's assumptions.

## Acceptance

Scaffolding only. The conductor + `McpOverAcpPolyfill` chain is installed
(`src/acp/core/actor/mod.rs`), but the polyfill stays inert until the client
handlers land — see the follow-up story, which carries the unchecked items.

- [x] `agent-client-protocol-polyfill` added as a dependency
- [x] Conductor + polyfill inserted into the client→agent chain
- [x] Inline MCP path still works as a fallback
- [x] `cargo test -q --all-targets`, `cargo clippy -q --all-targets -- -D warnings`,
      `cargo fmt --check -q`, `make test-contract` pass
- [ ] → follow-up: MCP-over-ACP relay methods (`mcp/connect`, `mcp/message`,
      `mcp/disconnect`) wired in the actor loop
- [ ] → follow-up: agents without `mcpCapabilities.acp` bridged via HTTP
- [ ] → follow-up: `docs/STATUS.md` "MCP-over-ACP — broker not wired" gap resolved

## Suggested commit

```
feat(acp): wire MCP-over-ACP broker via agent-client-protocol-polyfill

Route MCP server attachment through the SDK's McpOverAcpPolyfill so
agents that support MCP-over-ACP get native mcp/connect/message/disconnect,
while agents that don't are transparently bridged via HTTP. Wire the
relay methods in the actor loop. Resolves the "broker not wired" gap.
```
