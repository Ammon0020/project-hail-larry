# ACP Rust SDK Reference

> **Crate:** `agent-client-protocol` v1.2.0 (schema crate
> `agent-client-protocol-schema` v1.4.0) — official SDK at
> [`agentclientprotocol/rust-sdk`](https://github.com/agentclientprotocol/rust-sdk).
> **Pinned & verified by S-ACP-SPIKE on 2026-07-15.** MSRV 1.92.0 compatible.
> Required features: `unstable` (provider mgmt, session fork) and
> `unstable_mcp_over_acp` (MCP relay — closes the Go SDK gap, see §MCP below).

This is the **critical dependency** — the entire `internal/acp/` package
(5,066 lines) is built on the Go SDK (`coder/acp-go-sdk`). The official Rust
SDK makes the port feasible. The API surface below is **verified** by
`tests/spike_acp.rs` (S-ACP-SPIKE) against the Rust `src/bin/mockagent.rs`
fixture;
earlier "conceptual mapping notes" have been replaced with the real shapes.

## What the Go SDK Provides (current usage)

The Go code uses `coder/acp-go-sdk` for:
- `Client` interface — the app implements the **client side** (owns fs/shell/permissions)
- `ClientSideConnection` — manages the JSON-RPC transport over stdio to agent subprocesses
- Session lifecycle: `initialize`, `session/new`, `session/load`, `session/list`, `session/prompt`, `session/cancel`, `session/update` (streaming notifications)
- Permission requests: `session/request_permission` → client responds with allow/deny
- Tool callbacks: `ReadTextFile`, `WriteTextFile`, `ExecuteCommand` (shell), terminal ops
- Provider management: `providers/list`, `providers/set`, `providers/disable` (unstable)
- MCP relay: `mcp/connect`, `mcp/disconnect` (the `mcp/message` relay is a known SDK gap)

## Verified Rust SDK API Surface (S-ACP-SPIKE, 2026-07-15)

The Rust SDK uses a **builder + handler** pattern rather than Go's interface
implementation model. All calls are async (tokio). The transport is built
from `(AsyncWrite, AsyncRead)` pairs using **`futures::io`** traits (not
tokio's `AsyncRead`/`AsyncWrite` — see Transport below).

### Roles & entry point

```rust
use agent_client_protocol::{Client, Agent, ConnectionTo};
// `Client` and `Agent` are unit structs. `Client::builder()` returns
// `Builder<Client, NullHandler, NullRun>` pre-configured for v1.
Client.builder()
    .name("local-agent")
    // register handlers, then:
    .connect_with(transport, async |cx: ConnectionTo<Agent>| { ... })  // -> Result<R, Error>
    .await?;
```

### Handler registration (replaces Go's `Client` interface)

```rust
// Typed request handler — `responder.respond(value)` is the single answer
// channel (the "first response wins" contract).
.on_receive_request(
    async |req: ReadTextFileRequest, responder, _cx: ConnectionTo<Agent>| {
        responder.respond(ReadTextFileResponse::new(contents))
    },
    agent_client_protocol::on_receive_request!(),  // return-type-notation hack
)
// Typed notification handler (no response).
.on_receive_notification(
    async |notif: SessionNotification, _cx: ConnectionTo<Agent>| { Ok(()) },
    agent_client_protocol::on_receive_notification!(),
)
```

The `on_receive_request!()` / `on_receive_notification!()` macros are
required workaround arguments for the lack of return-type notation
(rust-lang/rust#109417). They expand to a `Box::pin` wrapper.

### Sending requests / notifications

```rust
// `cx: ConnectionTo<Agent>`
let init = cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
    .block_task()   // spawns a task to await without deadlocking the dispatch loop
    .await?;
cx.send_notification(CancelNotification::new(session_id))?;
```

`SentRequest<T>` (returned by `send_request`) has:
- `.block_task().await` — await inside a spawned task (preferred)
- `.detach()` — keep the request running on the peer, ignore the response
- `.cancel()` — send `$/cancel_request`
- `Drop` — auto-sends `$/cancel_request` and discards the response

### Transport — Byte Stream (stdio to subprocess)

The SDK uses **`futures::io::{AsyncRead, AsyncWrite}`** (futures-io), NOT
tokio's. Two verified stdio paths:

1. **`AcpAgent`** (blessed, recommended): parses a command string, spawns
   the child with `async_process::Command`, pipes stdin/stdout, and adapts
   to the line-delimited JSON-RPC transport. Wraps the child in a
   `ChildGuard` that **kills the process on drop** (process-tree
   termination contract).
   ```rust
   use std::str::FromStr;
   // `/tmp/mockagent` is where CI copies the Rust binary built from
   // `src/bin/mockagent.rs`; implements ConnectTo<Client>.
   let agent = AcpAgent::from_str("/tmp/mockagent")?;
   Client.builder().connect_with(agent, |cx| async { ... }).await?;
   ```

2. **`ByteStreams`** (manual, for PID tracking / custom spawn): wraps an
   `(AsyncWrite, AsyncRead)` pair. Use `async_process::Command` (NOT
   `tokio::process::Command`) so the streams implement `futures::io` traits.
   ```rust
   use agent_client_protocol::ByteStreams;
   let transport = ByteStreams::new(child_stdin, child_stdout);  // async_process::ChildStd{in,out}
   ```

### Schema types — generated, strongly typed

All request/response/notification types live in
`agent_client_protocol::schema::v1::*` and are serde-derived. Key types
verified by the spike:

| Operation | Request | Response / Notification |
|---|---|---|
| `initialize` | `InitializeRequest::new(ProtocolVersion::V1)` | `InitializeResponse { protocol_version, agent_capabilities, auth_methods, agent_info, .. }` |
| `session/new` | `NewSessionRequest::new(cwd: PathBuf)` | `NewSessionResponse { session_id: SessionId, .. }` |
| `session/prompt` | `PromptRequest::new(session_id, Vec<ContentBlock>)` | `PromptResponse { stop_reason: StopReason, .. }` |
| `session/cancel` | `CancelNotification::new(session_id)` | — (notification) |
| `session/update` | — (notification) | `SessionNotification { session_id, update: SessionUpdate, .. }` |
| `fs/read_text_file` | `ReadTextFileRequest::new(session_id, path)` | `ReadTextFileResponse { content, .. }` |
| `fs/write_text_file` | `WriteTextFileRequest::new(session_id, path, content)` | `WriteTextFileResponse` (unit-ish) |
| `session/request_permission` | `RequestPermissionRequest { session_id, tool_call, options }` | `RequestPermissionResponse { outcome: RequestPermissionOutcome }` |
| `authenticate` | `AuthenticateRequest::new(method_id)` | `AuthenticateResponse` (default) |
| `terminal/create` | `CreateTerminalRequest::new(session_id, command)` | `CreateTerminalResponse { terminal_id }` |
| `terminal/output` | `TerminalOutputRequest::new(session_id, terminal_id)` | `TerminalOutputResponse { output, truncated, exit_status }` |
| `terminal/release`/`kill`/`wait_for_exit` | `::new(session_id, terminal_id)` | (default-constructible) |

`SessionUpdate` is a typed enum: `AgentMessageChunk(ContentChunk)`,
`AgentThoughtChunk(ContentChunk)`, `ToolCall(ToolCall)`,
`ToolCallUpdate(ToolCallUpdate)`, `Plan(Plan)`, `UsageUpdate`, etc.
`ContentChunk.content` is a `ContentBlock` enum (`Text`/`Image`/`Audio`/
`ResourceLink`/`Resource`). `StopReason` is `EndTurn`/`MaxTokens`/
`MaxTurnRequests`/`Refusal`/`Cancelled`.

`SessionId` is a `#[serde(transparent)]` newtype around `Arc<str>` with
`Display` + `From<&str, String, Arc<str>>`; access the inner via `.0`.

### Permission response shape

```rust
RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
    SelectedPermissionOutcome::new(option_id),  // echoes the chosen PermissionOptionId
));
// or
RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled);
```

## Migration Notes

1. **No `Client` interface to implement** — the Go SDK has the app implement
   the `Client` interface (callbacks for file reads, writes, shell). The Rust
   SDK inverts this: you register handlers via `.on_receive_request()`. The
   logic in `transport.go` (`acpClientImpl`) maps to these handlers.

2. **Streaming via notifications** — `SessionNotification` carries
   `SessionUpdate` with typed update kinds. The Go code translates these in
   `messages.go`; the Rust SDK provides typed enums instead of raw JSON maps.

3. **Schema types are generated** — strongly typed (serde-derived). More
   ergonomic than Go's `map[string]interface{}` but requires matching exact
   variant names. All builders use `::new(...)` with required fields; optional
   fields via chained builder methods (no `.builder().build()` pattern).

4. **Async-native** — built on `tokio` for the runtime, but the transport
   layer uses `futures::io` traits. `async_process` (not `tokio::process`)
   provides the stdio streams that implement those traits. `serve_with()` is
   replaced by `connect_with(transport, main_fn)` which runs the dispatch
   loop until `main_fn` returns.

5. **MCP relay — GAP CLOSED.** With the `unstable_mcp_over_acp` feature, the
   Rust SDK **fully code-generates** `mcp/connect`, `mcp/message` (request,
   notification, and response), and `mcp/disconnect`. This closes the Go SDK
   gap (Go only had `mcp/connect`/`disconnect` and couldn't relay
   `mcp/message`, forcing the inline transport workaround in
   `acp-spec-compliance.md` §4.10). **The Rust port can drop that
   workaround.**
   - **v1 dispatch caveat:** for protocol v1, the standalone MCP types
     (`ConnectMcpRequest`, `MessageMcpRequest`, etc.) do **not** implement
     `JsonRpcMessage` directly — they are only reachable as variants of the
     `AgentRequest` / `AgentResponse` / `AgentNotification` enums. Send via
     `cx.send_request::<AgentRequest>(AgentRequest::ConnectMcpRequest(...))`
     and match the response enum. (Standalone `JsonRpcMessage` impls exist
     only for the v2 draft.) The `on_receive_request::<AgentRequest>`
     pattern dispatches all agent→client requests in one closure.

6. **No `ExecuteCommand` request.** ACP delegates shell execution to the
   `terminal/*` family (`terminal/create` + `terminal/output` +
   `terminal/release`/`kill`/`wait_for_exit`). The Go code's
   `ExecuteCommand` callback is a higher-level helper wrapping these; in
   Rust, S-SHELL + S-ACP-CORE will register a `CreateTerminalRequest`
   handler and drive it.

7. **PKCE auth** — the SDK surfaces `InitializeResponse.auth_methods:
   Vec<AuthMethod>` (variants: `Agent` default, `EnvVar`/`Terminal` behind
   `unstable_auth_methods`) and `AuthenticateRequest`/`AuthenticateResponse`
   + `LogoutRequest`/`LogoutResponse`. The mockagent returns empty
   `auth_methods` and a no-op `Authenticate`; the spike verified the
   `authenticate` round-trip compiles and succeeds. A real PKCE dance
   (OAuth redirect, code exchange) requires a real agent and is deferred to
   S-ACP-CORE.

8. **Cancellation & process tree** — dropping a `SentRequest` sends
   `$/cancel_request`. Tearing down the connection (returning from
   `connect_with`'s main closure, or erroring) drops the transport, which
   drops `AcpAgent`'s `ChildGuard` → `child.kill()` (or `kill_on_drop` on a
   manual `async_process::Child`). The client owns the child process tree;
   the mockagent's `Cancel` handler is a no-op, confirming the production
   contract: client-side teardown is the cancellation guarantee.

## Fetching Live Docs

```
context7: resolve-library-id "agent-client-protocol rust-sdk"
context7: query-docs /agentclientprotocol/rust-sdk "<specific question>"
```

Useful queries: "client side connection", "session prompt streaming",
"permission request respond", "tool call read file write file",
"transport stdio subprocess", "provider list set disable".

## Spike verification artifact

`tests/spike_acp.rs` (S-ACP-SPIKE) — 7 passing integration tests covering
initialize, session/new, prompt streaming, file/shell callback type
reachability, permission response shape, cancellation + child teardown, MCP
relay type support, and auth flow shape. Run with:
`cargo build --bin mockagent && cp target/debug/mockagent /tmp/mockagent && cargo test --test spike_acp -- --nocapture`
(CI copies the Rust binary built from `src/bin/mockagent.rs` to `/tmp/mockagent`,
which is the spawn path used by the tests above.)
