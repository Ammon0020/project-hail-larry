# ACP Rust SDK Reference

> **Crate:** `agent_client_protocol` — official SDK at
> [`agentclientprotocol/rust-sdk`](https://github.com/agentclientprotocol/rust-sdk)
> Context7 ID: `/agentclientprotocol/rust-sdk` (305 snippets, High reputation)

This is the **critical dependency** — the entire `internal/acp/` package
(5,066 lines) is built on the Go SDK (`coder/acp-go-sdk`). An official Rust
SDK makes the port feasible, but its multi-crate API and generated schema can
change quickly. The snippets below are conceptual mapping notes only; S-ACP-
SPIKE must verify the current crates, MSRV, process/stdio helpers, auth flow,
and MCP relay before implementation. The transport requires a rewrite, not a
line-by-line port.

## What the Go SDK Provides (current usage)

The Go code uses `coder/acp-go-sdk` for:
- `Client` interface — the app implements the **client side** (owns fs/shell/permissions)
- `ClientSideConnection` — manages the JSON-RPC transport over stdio to agent subprocesses
- Session lifecycle: `initialize`, `session/new`, `session/load`, `session/list`, `session/prompt`, `session/cancel`, `session/update` (streaming notifications)
- Permission requests: `session/request_permission` → client responds with allow/deny
- Tool callbacks: `ReadTextFile`, `WriteTextFile`, `ExecuteCommand` (shell), terminal ops
- Provider management: `providers/list`, `providers/set`, `providers/disable` (unstable)
- MCP relay: `mcp/connect`, `mcp/disconnect` (the `mcp/message` relay is a known SDK gap)

## Conceptual Rust SDK API Surface (verify in S-ACP-SPIKE)

The Rust SDK uses a **builder + handler** pattern rather than Go's interface
implementation model:

```rust
use agent_client_protocol::{Client, Agent, ConnectTo};
use agent_client_protocol::schema::{ProtocolVersion, v1::InitializeRequest};

// Client side: connect to an agent subprocess over stdio
Client.builder()
    .name("local-agent")
    .connect_with(transport, async |cx| {
        cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
            .block_task().await?;
        Ok(())
    }).await?;
```

### Transport — Byte Stream (stdio to subprocess)

```rust
// (AsyncWrite, AsyncRead) pair → JSON-RPC line-delimited transport
impl<OB: AsyncWrite, IB: AsyncRead> IntoJrConnectionTransport for (OB, IB) {
    fn setup_transport(self, cx, outgoing_rx, incoming_tx) -> Result<(), Error> {
        // Spawns: read bytes → parse JSON → incoming_tx
        // Spawns: outgoing_rx → serialize JSON → write bytes
    }
}
```

For stdio to a spawned agent process, use `tokio::process::Command` and pipe
`child.stdout` / `child.stdin` into the byte-stream transport.

### Handler-based Connection (replaces Go interface impls)

```rust
let connection = JrConnection::new()
    .name("local-agent")
    .on_receive_request(|req: ReadTextFileRequest, cx| {
        // Implement file read — return ReadTextFileResponse
        cx.respond(ReadTextFileResponse { /* ... */ })
    })
    .on_receive_request(|req: WriteTextFileRequest, cx| {
        // Implement file write with permission check
        cx.respond(WriteTextFileResponse { /* ... */ })
    })
    .on_receive_request(|req: ExecuteCommandRequest, cx| {
        // Shell execution — stream output via session/update notifications
        cx.respond(ExecuteCommandResponse { /* ... */ })
    })
    .on_receive_notification(|notif: SessionNotification, cx| {
        // Stream updates, tool calls, thoughts, plan updates → translate to events
        Ok(())
    });

connection.serve_with(transport).await?;
```

## Migration Notes

1. **No `Client` interface to implement** — the Go SDK has the app implement
   the `Client` interface (callbacks for file reads, writes, shell). The Rust
   SDK inverts this: you register handlers via `.on_receive_request()`. The
   logic in `transport.go` (`acpClientImpl`) maps to these handlers.

2. **Streaming via notifications** — `SessionNotification` carries
   `SessionUpdate` with typed update kinds (AgentMessageChunk,
   AgentThoughtChunk, ToolCallUpdate, PlanUpdate, etc.). The Go code
   translates these in `messages.go`; the Rust SDK provides typed enums
   instead of raw JSON maps.

3. **Schema types are generated** — the Rust SDK has a `schema` module with
   `v1` and `v2` protocol versions. All request/response/notification types
   are strongly typed (serde-derived). This is more ergonomic than Go's
   `map[string]interface{}` in places but requires matching the exact variant
   names.

4. **Async-native** — the Go SDK uses goroutines internally; the Rust SDK is
   built on `tokio`. All SDK calls are `async fn`. The transport's
   `serve_with()` runs the message loop as a future — wrap it in
   `tokio::spawn` per session.

5. **MCP relay gap** — the Go SDK doesn't code-generate `mcp/message` (only
   `mcp/connect`/`disconnect`), which is a known blocker (see STATUS.md).
   S-ACP-SPIKE must prove current Rust SDK relay support with an integration
   test; if absent, retain the inline transport workaround behind an isolated
   adapter rather than assuming schema coverage.

6. **PKCE auth** — the Go code does ACP agent auth (PKCE) in `auth_method`.
   The Rust SDK may handle this differently; verify the auth flow maps.

## Fetching Live Docs

```
context7: resolve-library-id "agent-client-protocol rust-sdk"
context7: query-docs /agentclientprotocol/rust-sdk "<specific question>"
```

Useful queries: "client side connection", "session prompt streaming",
"permission request respond", "tool call read file write file",
"transport stdio subprocess", "provider list set disable".
