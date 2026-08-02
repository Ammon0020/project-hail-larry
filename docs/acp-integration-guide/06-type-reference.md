# 06 — Type Reference

Quick reference for the SDK's key types, constructors, and enums. All types
live in `agent_client_protocol::schema::v1::*` unless noted. All constructors
use `::new(...)` for required fields and chained builder methods for optional
fields (no `.builder().build()` pattern).

## Top-level SDK types (`agent_client_protocol::*`)

| Type | What it is |
|------|-----------|
| `Client` | Unit struct marking the client role. `Client.builder()` → `Builder<Client, _, _>`. |
| `Agent` | Unit struct marking the agent role. Used as a type parameter: `ConnectionTo<Agent>`. |
| `ConnectionTo<R>` | Handle to the connection from role `R`'s peer. `ConnectionTo<Agent>` = "my (client) connection to the agent." Used to send requests/notifications. Cheap to clone. |
| `Builder` | Returned by `Client.builder()`. Chain `.name()`, `.on_receive_request()`, `.on_receive_notification()`, then `.connect_with(transport, main_fn)`. |
| `AcpAgent` | Blessed stdio transport. `AcpAgent::from_str(cmd)` spawns the child and implements `ConnectTo<Client>`. |
| `ByteStreams` | Manual transport wrapping `(AsyncWrite, AsyncRead)`. `ByteStreams::new(stdin, stdout)`. |
| `Responder<T>` | Passed to request handlers. Call `.respond(value)` or `.respond_with_internal_error(err)`. First response wins. |
| `SentRequest<T>` | Returned by `cx.send_request(req)`. Consume via `.block_task().await`, `.detach()`, `.cancel()`, or drop (auto-cancels). |
| `Error` | SDK error type. Has `.code: ErrorCode`. Construct via `Error::internal_error()`. |
| `JsonRpcRequest` | Derive macro (re-exported). `#[derive(JsonRpcRequest)]` + `#[request(method, response)]`. |
| `JsonRpcResponse` | Derive macro (re-exported). `#[derive(JsonRpcResponse)]`. Marks a type as a valid RPC response. |
| `on_receive_request!()` | Required macro arg for `.on_receive_request(handler, MACRO)`. |
| `on_receive_notification!()` | Required macro arg for `.on_receive_notification(handler, MACRO)`. |
| `schema::ProtocolVersion` | Enum: `ProtocolVersion::V1`. Passed to `InitializeRequest::new`. |
| `schema::v1::*` | All generated protocol types (see below). |

## Request / Response / Notification types

| Operation | Request | Response / Notification |
|-----------|---------|-------------------------|
| `initialize` | `InitializeRequest::new(ProtocolVersion::V1)` | `InitializeResponse { protocol_version, agent_capabilities, auth_methods, agent_info, .. }` |
| `session/new` | `NewSessionRequest::new(cwd: PathBuf)` | `NewSessionResponse { session_id: SessionId, config_options: Option<Vec<SessionConfigOption>>, .. }` |
| `session/load` | `LoadSessionRequest::new(session_id, cwd)` | `LoadSessionResponse { config_options, .. }` |
| `session/list` | `ListSessionsRequest::new().cwd(PathBuf)` | `ListSessionsResponse { sessions: Vec<SessionInfo>, .. }` |
| `session/prompt` | `PromptRequest::new(session_id, Vec<ContentBlock>)` | `PromptResponse { stop_reason: StopReason, .. }` |
| `session/cancel` | `CancelNotification::new(session_id)` | — (notification) |
| `session/update` | — (notification) | `SessionNotification { session_id, update: SessionUpdate, .. }` |
| `session/request_permission` | `RequestPermissionRequest { session_id, tool_call, options }` | `RequestPermissionResponse { outcome: RequestPermissionOutcome }` |
| `session/set_config_option` | `SetSessionConfigOptionRequest::new(session_id, config_id, value)` | (response type — unit-ish) |
| `fs/read_text_file` | `ReadTextFileRequest::new(session_id, path)` | `ReadTextFileResponse { content, .. }` |
| `fs/write_text_file` | `WriteTextFileRequest::new(session_id, path, content)` | `WriteTextFileResponse` (unit-ish) |
| `terminal/create` | `CreateTerminalRequest::new(session_id, command)` | `CreateTerminalResponse { terminal_id, .. }` |
| `terminal/output` | `TerminalOutputRequest::new(session_id, terminal_id)` | `TerminalOutputResponse { output, truncated, exit_status, .. }` |
| `terminal/wait_for_exit` | `WaitForTerminalExitRequest::new(session_id, terminal_id)` | `WaitForTerminalExitResponse { exit_status, .. }` |
| `terminal/kill` | `KillTerminalRequest::new(session_id, terminal_id)` | `KillTerminalResponse` (unit-ish) |
| `terminal/release` | `ReleaseTerminalRequest::new(session_id, terminal_id)` | `ReleaseTerminalResponse` (unit-ish) |
| `authenticate` | `AuthenticateRequest::new(method_id)` | `AuthenticateResponse` (default) |
| `mcp/connect` | `ConnectMcpRequest` (unstable_mcp_over_acp) | `ConnectMcpResponse` |
| `mcp/message` | `MessageMcpRequest` (unstable_mcp_over_acp) | `MessageMcpResponse` + `MessageMcpNotification` |
| `mcp/disconnect` | `DisconnectMcpRequest` (unstable_mcp_over_acp) | `DisconnectMcpResponse` |

### Builder method chains (optional fields)

```rust
InitializeRequest::new(ProtocolVersion::V1)
    .client_info(Implementation::new("name", "version"))
    .client_capabilities(
        ClientCapabilities::new()
            .fs(FileSystemCapabilities::new().read_text_file(true).write_text_file(true))
            .terminal(true),
    );

NewSessionRequest::new(cwd).mcp_servers(Vec<McpServer>);

LoadSessionRequest::new(session_id, cwd).mcp_servers(Vec<McpServer>);

ListSessionsRequest::new().cwd(PathBuf);

TerminalOutputResponse::new(output, truncated).exit_status(TerminalExitStatus::new().exit_code(u32).signal(u32));

TextResourceContents::new(text, uri).mime_type(String);
```

## Key newtypes

### `SessionId`

`#[serde(transparent)]` newtype around `Arc<str>`. Cheap to clone.

```rust
SessionId::new("acp-123");               // &str
SessionId::new(String::from("acp-123")); // String
SessionId::new(Arc::<str>::from("...")); // Arc<str>

id.to_string(); // -> String
&id.0;          // -> &Arc<str> (inner access)
```

Implements `Display`, `Clone`, `Eq`, `Hash`. Pass by value (`.clone()`) into
requests.

### `ConfigOptionId` / `ConfigOptionValue`

Similar `Arc<str>` newtypes. Access inner via `.0.as_ref()`.

## Key enums (all non-exhaustive — always include a fallback)

### `StopReason` (prompt response)

| Variant | String |
|---------|--------|
| `EndTurn` | `"end_turn"` |
| `MaxTokens` | `"max_tokens"` |
| `MaxTurnRequests` | `"max_turn_requests"` |
| `Refusal` | `"refusal"` |
| `Cancelled` | `"cancelled"` |
| `_` | `"unknown"` |

### `ToolKind` (tool call classification)

| Variant | String |
|---------|--------|
| `Read` | `"read"` |
| `Edit` | `"edit"` |
| `Delete` | `"delete"` |
| `Move` | `"move"` |
| `Search` | `"search"` |
| `Execute` | `"execute"` |
| `Think` | `"think"` |
| `Fetch` | `"fetch"` |
| `SwitchMode` | `"switch_mode"` |
| `Other` / `_` | `"other"` |

### `ToolCallStatus`

| Variant | String |
|---------|--------|
| `Pending` | `"pending"` |
| `InProgress` | `"in_progress"` |
| `Completed` | `"completed"` |
| `Failed` | `"failed"` |
| `_` | `"unknown"` |

### `PlanEntryStatus`

| Variant | String |
|---------|--------|
| `Pending` | `"pending"` |
| `InProgress` | `"in_progress"` |
| `Completed` | `"completed"` |
| `_` | `"unknown"` |

### `PermissionOptionKind`

| Variant | String |
|---------|--------|
| `AllowOnce` | `"allow_once"` |
| `AllowAlways` | `"allow_always"` |
| `RejectOnce` | `"reject_once"` |
| `RejectAlways` | `"reject_always"` |
| `_` | `"unknown"` |

### `RequestPermissionOutcome`

```rust
RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id))
RequestPermissionOutcome::Cancelled
```

### `SessionUpdate` (streaming notification payload)

| Variant | Payload | Notes |
|---------|---------|-------|
| `AgentMessageChunk(ContentChunk)` | agent text | stream to UI |
| `AgentThoughtChunk(ContentChunk)` | agent thought | stream to UI (thought mode) |
| `ToolCall(ToolCall)` | tool started | emit ToolStarted event |
| `ToolCallUpdate(ToolCallUpdate)` | tool progress/result | emit ToolCompleted event |
| `Plan(Plan)` | plan update | emit PlanUpdated event |
| `UserMessageChunk(ContentChunk)` | user text echo | **ignore** (duplicates your local PromptSubmitted) |
| `AvailableCommandsUpdate` | — | unsupported → log kind |
| `CurrentModeUpdate` | — | unsupported → log kind |
| `ConfigOptionUpdate` | — | unsupported → log kind |
| `SessionInfoUpdate` | — | unsupported → log kind |
| `UsageUpdate` | — | unsupported → log kind |
| `_` | — | non-exhaustive fallback → log kind |

### `ContentBlock` (content inside chunks / prompt blocks)

| Variant | Payload |
|---------|---------|
| `Text(TextContent)` | `.text: String` |
| `Image` | image data |
| `Audio` | audio data |
| `ResourceLink` | resource reference |
| `Resource(EmbeddedResource)` | embedded resource (attachments) |
| `_` | non-exhaustive |

### `ToolCallContent` (tool result blocks)

| Variant | Payload |
|---------|---------|
| `Content(ContentBlock)` | text/other result |
| `Diff(DiffContent)` | `.path`, `.old_text: Option<String>`, `.new_text: String` |
| `Terminal(TerminalContent)` | `.terminal_id` |
| `_` | non-exhaustive |

### `ToolCall` vs `ToolCallUpdate` (read carefully)

ACP has two tool-call shapes that share field names but differ in optionality
and access path:

| | `ToolCall` | `ToolCallUpdate` |
|---|-----------|-------------------|
| Carried by | `SessionUpdate::ToolCall` | `SessionUpdate::ToolCallUpdate`, `RequestPermissionRequest.tool_call` |
| Access | direct: `tc.title`, `tc.kind` | via `.fields`: `upd.fields.title`, `upd.fields.kind` |
| `title` | `String` (required) | `Option<String>` |
| `kind` | `ToolKind` (default) | `Option<ToolKind>` |
| `status` | `ToolCallStatus` (default) | `Option<ToolCallStatus>` |
| `locations` | `Vec<ToolCallLocation>` | `Option<Vec<ToolCallLocation>>` |
| `content` | `Vec<ToolCallContent>` | `Option<Vec<ToolCallContent>>` |
| `raw_input` | `Option<serde_json::Value>` | `Option<serde_json::Value>` |
| `raw_output` | `Option<serde_json::Value>` | `Option<serde_json::Value>` |
| `tool_call_id` | `ToolCallId` | `ToolCallId` |

`ToolCallUpdateFields` is `#[serde(flatten)]` into `ToolCallUpdate`, so on the
wire the fields appear at the same level as `tool_call_id`, but in Rust you
access them through `.fields`.

### `ErrorCode` (JSON-RPC error codes)

| Variant | Label |
|---------|-------|
| `ParseError` | `"parse_error"` |
| `InvalidRequest` | `"invalid_request"` |
| `MethodNotFound` | `"method_not_found"` |
| `InvalidParams` | `"invalid_params"` |
| `InternalError` | `"internal_error"` |
| `RequestCancelled` | `"request_cancelled"` |
| `AuthRequired` | `"auth_required"` |
| `ResourceNotFound` | `"resource_not_found"` |
| `Other(_)` | `"other"` |
| `_` | `"unknown"` |

## Initialize capability types

```rust
// InitializeResponse.agent_capabilities: AgentCapabilities
//   .providers: Option<ProvidersCapability>  // Some = providers supported
//   .load_session: bool
//   .session_capabilities: SessionCapabilities
//     .list: Option<...>     // Some = can list
//     .resume: Option<...>   // Some = can resume
//     .close: Option<...>    // Some = can close
//     .delete: Option<...>   // Some = can delete
//   .prompt_capabilities: PromptCapabilities
//     .embedded_context: bool
//   .mcp_capabilities: McpCapabilities
//     .http: bool, .sse: bool, (stdio always)
//   .auth_methods: Vec<AuthMethod>  // (behind unstable_auth_methods)

// ClientCapabilities::new()
//   .fs(FileSystemCapabilities::new().read_text_file(bool).write_text_file(bool))
//   .terminal(bool)

// McpServer::Stdio(StdioMcpServer { name, command, args, env })
// McpServer::Http(HttpMcpServer { name, url, .. })  // requires caps.http
// McpServer::Sse(SseMcpServer { name, url, .. })    // requires caps.sse
```
