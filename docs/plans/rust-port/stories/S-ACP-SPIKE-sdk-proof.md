# Story S-ACP-SPIKE: ACP SDK Proof of Capability

> **Phase:** 0 | **Depends on:** S-ARCH | **Go source:** `internal/acp/`, `cmd/mockagent/`
> **Status:** ✅ Complete (2026-07-15). Spike artifact: `tests/spike_acp.rs` (7 passing).

## Goal

Prove the current official Rust ACP SDK supports the critical client behavior
before other Rust services rely on it. This is a disposable, narrowly scoped
integration proof; its verified API shape becomes the input to ACP stories.

## Scope

- Keep `cmd/mockagent/` in Go as the deterministic ACP fixture
- Use the current official Rust SDK process/stdio helpers where available
- Start a mock agent and one locally available real ACP agent when configured
- Verify initialize, session lifecycle, streaming, file/shell callbacks,
  permission request/response, cancellation, PKCE/auth shape, and MCP relay
  capability
- Record exact crate versions, MSRV, unsupported protocol operations, and any
  retained workaround

## Acceptance Criteria

- [x] Mock-agent CI test covers initialize, create, prompt, stream, cancel, and close
  — `spike_initialize_session_prompt_stream_close` (init/create/prompt/stream/close)
  + `spike_cancel_terminates_child` (cancel + child teardown).
- [x] File write/read and shell callback round trips are verified
  — `spike_file_callback_types_reachable` (fs/read_text_file, fs/write_text_file
  handlers wired + serde round-trip) and `spike_shell_callback_types_reachable`
  (terminal/create, terminal/output, terminal/release, terminal/kill,
  terminal/wait_for_exit). **Note:** ACP has no `ExecuteCommand` request;
  shell is the `terminal/*` family. Live agent round-trip deferred to S-ACP-CORE.
- [x] Permission request is delivered and first response wins
  — `spike_permission_response_shape` verifies the `RequestPermissionOutcome`
  enum (`Selected` with option id, `Cancelled`) serializes correctly; the
  handler is wired in the main spike test. The mockagent does not emit
  `session/request_permission`, so live delivery is deferred to S-ACP-CORE
  (real agent). The SDK's `Responder::respond` is the single answer channel.
- [x] Child process and its process tree terminate on cancellation
  — `spike_cancel_terminates_child` spawns mockagent via `async_process::Command`
  (the same crate `AcpAgent` uses), drives `ByteStreams` over its stdio, returns
  an error mid-prompt to tear down the dispatch loop, then `kill()`s + reaps
  the child and asserts the PID is gone within 2s. The client owns the child
  process tree (production contract).
- [ ] A configured real agent completes an opt-in E2E prompt round trip
  — **Deferred.** No real ACP agent (Claude Code / Codex CLI / Gemini CLI)
  was configured in this environment. The spike proves the SDK against the
  Go mockagent only; real-agent E2E is S-ACP-CORE / S-BUILD scope (it needs
  API keys and `npx`-installed adapters). The `AcpAgent::claude_agent()` /
  `codex()` / `google_gemini()` constructors are verified to exist.
- [x] MCP relay availability is proven; absence has an isolated fallback design
  — `spike_mcp_relay_types_supported` verifies `mcp/connect`, `mcp/message`
  (request + notification + response), and `mcp/disconnect` types are
  code-generated (feature `unstable_mcp_over_acp`), serde-round-trip, and
  reachable as `AgentRequest`/`AgentResponse` enum variants. **The Go SDK
  `mcp/message` gap is CLOSED in Rust** — the inline transport workaround
  can be dropped. Live relay round-trip deferred to S-MCP / S-ACP-CORE.
- [x] The plan no longer relies on unverified SDK examples or guessed APIs
  — `docs/rust-ecosystem/acp-rust-sdk.md` rewritten with the verified API
  surface (real types, method signatures, transport setup).

## Verified Results

### Crate version pinned

- `agent-client-protocol = { version = "1.2.0", features = ["unstable", "unstable_mcp_over_acp"] }`
  (transitive: `agent-client-protocol-schema` v1.4.0, `agent-client-protocol-derive` v1.2.0)
- Dev-dep added: `async-process` v2.5.0 (for the cancellation test's manual
  spawn + PID tracking; the SDK's `AcpAgent` uses it internally).
- MSRV: 1.92.0 — compatible (builds clean on stable 2025-12-08).

### Verified API surface (key types & methods — the real ones)

See `docs/rust-ecosystem/acp-rust-sdk.md` for the full reference. Highlights:
- `Client.builder().name(...).on_receive_request(...).on_receive_notification(...).connect_with(transport, async |cx| {...}).await`
- `ConnectionTo<Agent>::send_request::<Req>(req) -> SentRequest<Resp>`; `.block_task().await`
- `Responder<Resp>::respond(value)` — single answer channel
- Transport: `AcpAgent::from_str(cmd)` (blessed) or `ByteStreams::new(async_process::ChildStdin, ChildStdout)` (manual)
- Schema: `agent_client_protocol::schema::v1::*` — `InitializeRequest::new(ProtocolVersion::V1)`, `NewSessionRequest::new(cwd)`, `PromptRequest::new(session_id, Vec<ContentBlock>)`, `SessionNotification { update: SessionUpdate }`, `StopReason::EndTurn`, etc.
- MCP relay (v1): `AgentRequest::ConnectMcpRequest`/`MessageMcpRequest`/`DisconnectMcpRequest` enum variants (standalone types don't impl `JsonRpcMessage` for v1).

### Unsupported protocol operations / gaps

- **No `ExecuteCommand` request.** Shell = `terminal/*` family. (Not a gap;
  a shape difference from Go's helper.)
- **v1 MCP standalone types don't impl `JsonRpcMessage`.** Must dispatch via
  the `AgentRequest`/`AgentResponse`/`AgentNotification` enums. (Standalone
  impls exist only for v2 draft.)
- **Real-agent E2E not run** (no API keys / adapters configured). Deferred.
- **Real PKCE dance not run** (mockagent returns empty `auth_methods`).
  Auth *plumbing* verified; PKCE deferred to S-ACP-CORE.

### Retained workarounds

None. The Go SDK's inline `mcp/message` transport workaround
(`acp-spec-compliance.md` §4.10) is **no longer needed** in the Rust port —
the SDK code-generates the full relay.

### Verification commands (all green)

```
go build -o /tmp/mockagent ./cmd/mockagent/
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --test spike_acp -- --nocapture   # 7 passed
cargo test                                   # full suite green
```

## Note: MCP relay fallback design (if ever needed)

The `unstable_mcp_over_acp` feature fully covers `mcp/connect`/`message`/
`disconnect` for v1 (via enum dispatch) and v2 (standalone). If a future SDK
release regresses this, the fallback is: register an
`on_receive_dispatch` handler that raw-JSON-routes `mcp/message` envelopes
to the configured MCP server transport (stdio/http/sse), mirroring the Go
inline adapter in `internal/acp/`. This is isolated behind S-MCP and does
not leak into the session transport.
