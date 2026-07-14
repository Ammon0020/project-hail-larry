# Story S-ACP: ACP Client (Rust SDK)

> **Phase:** 3 | **Depends on:** S-INTERFACES, S-SHELL, S-FILES, S-PERMISSIONS
> **Go source:** `internal/acp/` (5,066 lines) — **the largest package**

## Summary

Port the ACP client layer using the official Rust SDK
(`agent_client_protocol` / `agentclientprotocol/rust-sdk`). This is the
biggest and most complex story — it handles all agent communication:
session lifecycle, prompt streaming, tool callbacks (file read/write,
shell exec, terminal), permission relay, provider management, autodetect,
agent registry, conversation history, context management.

## Go Source (12 files)

| File | Responsibility |
|---|---|
| `acp.go` | `Client` struct, `ClientConfig`, session management, lifecycle |
| `transport.go` | `acpClientImpl` — implements Go SDK `Client` interface (file/shell/term callbacks) |
| `messages.go` | ACP message → event translation (stream updates, tool calls, plans) |
| `conversation.go` | Conversation history, export, rebind |
| `context.go` | Open-files context, session context management |
| `providers.go` | Provider list/set/disable (unstable ACP capability) |
| `agent_registry.go` | Registered agents (name, command, models), add/remove |
| `autodetect.go` | Probe known CLIs (claude, codex, gemini, etc.) for ACP support |
| `terminal.go` | Terminal session management (ACP terminal tool) |
| `profile.go` | Agent modes (Ask/Plan/Agent) |
| `store.go` | Session metadata persistence |
| `ringbuffer.go` | Ring buffer for recent output |

## Rust Implementation

### Use the official Rust SDK — see `docs/rust-ecosystem/acp-rust-sdk.md`

The Go SDK has the app implement a `Client` interface (callbacks for
`ReadTextFile`, `WriteTextFile`, `ExecuteCommand`, terminal ops). The
Rust SDK uses handler registration:

```rust
let connection = JrConnection::new()
    .on_receive_request(|req: ReadTextFileRequest, cx| { /* ... */ })
    .on_receive_request(|req: WriteTextFileRequest, cx| { /* ... */ })
    .on_receive_request(|req: ExecuteCommandRequest, cx| { /* ... */ })
    .on_receive_notification(|notif: SessionNotification, cx| { /* ... */ });
```

### Sub-modules

- `acp::client` — `AcpClient` struct, session map, `ClientConfig`
- `acp::transport` — handler implementations (file ops → S-FILES, shell →
  S-SHELL, permissions → S-PERMISSIONS)
- `acp::messages` — `SessionNotification` → `Event` translation
- `acp::conversation` — history, export, rebind
- `acp::context` — open-files tracker
- `acp::providers` — provider management via SDK
- `acp::agent_registry` — `DashMap<String, AgentInfo>`
- `acp::autodetect` — `tokio::process::Command` to probe CLIs
- `acp::terminal` — terminal session map
- `acp::profile` — mode enum + config option
- `acp::store` — session metadata (SQLite or JSON)

### Key migration concerns

1. **API surface differs** — see acp-rust-sdk.md. The handler pattern
   inverts the Go interface model. Logic is the same; wiring changes.
2. **Streaming** — `SessionNotification` carries typed `SessionUpdate`
   variants. Map each to the corresponding `Event` type (the Go code does
   this in `messages.go` with raw JSON — Rust SDK gives typed enums).
3. **MCP relay gap** — verify if Rust SDK has `mcp/message` (Go SDK doesn't).
   If it does, the inline transport workaround can be removed.
4. **PKCE auth** — port `auth_method` logic; verify SDK auth flow.
5. **Spawn agent process** — `tokio::process::Command`, pipe stdin/stdout
   to the SDK's byte-stream transport.

### This is the highest-risk story

Allocate the most time here. Consider splitting into sub-stories:
- S-ACP-CORE: client struct, session lifecycle, transport handlers
- S-ACP-STREAM: message translation, streaming
- S-ACP-PROVIDERS: provider management
- S-ACP-AUTODETECT: agent probing
- S-ACP-CONVERSATION: history, export, rebind, context

## Acceptance Criteria

- [ ] Agent process spawns and initializes via Rust SDK
- [ ] `SendPrompt` streams responses → events
- [ ] File read/write tool callbacks work (via S-FILES)
- [ ] Shell exec tool callback works (via S-SHELL)
- [ ] Permission requests relayed (via S-PERMISSIONS)
- [ ] Session create/load/list/close work
- [ ] Provider list/set/disable work
- [ ] Autodetect finds known agents
- [ ] Cancellation works (session/cancel)
- [ ] `cargo test acp` passes (port integration_test.go)
- [ ] E2E: real agent (claude/codex) completes a prompt round-trip
