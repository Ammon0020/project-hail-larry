# Wire mcp/connect, mcp/message, mcp/disconnect handlers + emit McpServer::Acp

> **Status:** pending | **Difficulty:** hard | **Urgency:** high
> **Follow-up to:** `complete-mcp-over-acp-polyfill-hard-high.md`

## Goal

Complete the MCP-over-ACP polyfill integration by wiring the three missing
client-side handlers and emitting `McpServer::Acp` from `ServerConfig::to_acp`
when the agent advertises `mcpCapabilities.acp`. The polyfill is currently
inert scaffolding — this story makes it functional.

## Background

The conductor + `McpOverAcpPolyfill` chain is installed in
`src/acp/core/actor/mod.rs` (commit pending). The polyfill:

1. Flips `mcpCapabilities.acp = true` on init (HttpAdapter mode)
2. Transforms `McpServer::Acp` declarations into loopback HTTP bridges
3. Sends `mcp/connect`, `mcp/message`, `mcp/disconnect` requests **to the
   client** (the daemon) when bridging

But two pieces are missing:

- **No client-side handlers**: The `SdkClient` builder registers handlers for
  `ReadTextFile`, `WriteTextFile`, terminal ops, `RequestPermission`, and
  `SessionNotification` — but not for `ConnectMcpRequest`, `MessageMcpRequest`,
  or `DisconnectMcpRequest`. Without these, the SDK returns method-not-found
  and the polyfill tears down the bridge.
- **No `McpServer::Acp` emission**: `ServerConfig::to_acp` (`src/mcp/mod.rs`)
  only emits `Stdio`/`Http`/`Sse`. The polyfill only transforms `Acp`
  declarations, so nothing is ever bridged.

## Approach

1. **Wire `on_receive_request` handlers** in the `SdkClient` builder
   (`src/acp/core/actor/mod.rs`, around line 274-422) for:
   - `ConnectMcpRequest` — accept the MCP connection, spawn/identify the local
     MCP server process, return `ConnectMcpResponse`
   - `MessageMcpRequest` — relay the JSON-RPC message to the local MCP server
     and return the response
   - `DisconnectMcpRequest` — tear down the local MCP connection, return
     `DisconnectMcpResponse`

   These handlers run on the daemon side; the polyfill routes MCP traffic
   through them via the conductor's proxy chain.

2. **Emit `McpServer::Acp`** from `ServerConfig::to_acp` (`src/mcp/mod.rs`)
   when `caps.acp` is true. Add a new `Transport::Acp` variant (or a condition
   on the existing transport selection) that produces
   `McpServer::Acp(McpServerAcp::new(name, server_id))`. The polyfill will
   then bridge it via loopback HTTP for agents that don't natively support
   ACP, and pass it through for agents that do.

3. **Update `load_session_mcp_servers`** (`src/acp/core/mcp.rs`) to consult
   `caps.acp` when filtering servers.

4. **Add a smoke test** that exercises the conductor chain — at minimum,
   verify the init handshake succeeds with the conductor in place and that
   an `McpServer::Acp` declaration is correctly transformed.

5. **Security note**: Document in `docs/STATUS.md` or a review file that the
   loopback HTTP bridge has no transport-level auth (consistent with MCP spec
   and the daemon's loopback trust model) and that listener count is unbounded
   (low risk — bounded by session MCP server count).

## Risks

- **Handler complexity**: The `mcp/message` handler must relay arbitrary
  JSON-RPC to a local MCP server process and await its response. This is
  essentially an MCP client. Consider whether the existing `src/mcp/` module
  can be reused or if a new lightweight relay is needed.
- **Lifecycle management**: Each `mcp/connect` spawns a local MCP server;
  `mcp/disconnect` must kill it. Track connections by `McpConnectionId`.
- **Concurrency**: Multiple simultaneous `mcp/message` calls to the same
  server must be multiplexed. The MCP JSON-RPC protocol already supports
  this, but the relay must handle concurrent requests correctly.

## Dependencies

- Requires the conductor + polyfill scaffolding (done — see
  `complete-mcp-over-acp-polyfill-hard-high.md`).
- Coordinate with `active-profile-mcp-transition-hard-high.md` — live MCP
  add/remove changes the transition dialog's assumptions.

## Acceptance

- [ ] `ConnectMcpRequest`, `MessageMcpRequest`, `DisconnectMcpRequest`
      handlers are wired in the actor loop
- [ ] `ServerConfig::to_acp` emits `McpServer::Acp` when `caps.acp` is true
- [ ] `load_session_mcp_servers` consults `caps.acp`
- [ ] Smoke test covers the conductor + polyfill chain
- [ ] Security note documented (loopback bridge auth + listener cap)
- [ ] `make check` passes
