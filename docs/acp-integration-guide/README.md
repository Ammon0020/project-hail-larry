# ACP Rust SDK Integration Guide

> **Audience:** Developers integrating the `agent-client-protocol` Rust crate into a
> new application, **without internet access**. This guide is self-contained: it
> documents the API surface, patterns, and pitfalls verified against a real
> production codebase. No external lookups required.
>
> **Source of truth:** Every code pattern below is extracted from a live
> production daemon that uses the SDK as its sole agent-integration boundary.
> The patterns were verified by an integration test spike (`tests/spike_acp.rs`)
> against a mock agent binary, and by the production actor in
> `src/acp/core/actor/`.

## What is ACP?

The **Agent Client Protocol (ACP)** is a JSON-RPC 2.0 protocol between an
**agent** (an AI coding assistant CLI: Claude Code, Codex, Gemini, etc.) and a
**client** (the application that owns the filesystem, shell, permissions, and
UI). The client spawns the agent as a subprocess and speaks ACP over the
process's stdin/stdout. The agent streams responses and proposes tool actions
(file reads/writes, shell commands); the client executes approved actions and
returns results.

Two roles:
- **Client** — owns fs/shell/permissions/sessions. You implement this side.
- **Agent** — the AI harness subprocess. You spawn it and talk to it.

The Rust SDK (`agent-client-protocol`) provides the client-side machinery:
transport, JSON-RPC dispatch, typed schema, and a builder/handler pattern for
registering callbacks.

## Crate versions used

```toml
# Cargo.toml
agent-client-protocol = { version = "1.3.0", features = ["unstable", "unstable_mcp_over_acp"] }
agent-client-protocol-schema = { version = "1.4.0", features = ["unstable_llm_providers"] }
# Transport: async_process owns the agent child + provides futures::io streams
async-process = "2.5.0"
futures-util = "0.3"
```

**Why two crates?** `agent-client-protocol` is the SDK (transport, dispatch,
builder). `agent-client-protocol-schema` is the generated typed schema. The SDK
re-exports the schema under `agent_client_protocol::schema::v1::*`, but some
unstable schema features (notably `unstable_llm_providers`) are **not forwarded**
by the SDK's `unstable` umbrella, so a direct schema dep is required for Cargo
feature unification to keep those types from being stripped. See
[05-providers-and-config.md](05-providers-and-config.md).

**Feature flags:**
- `unstable` — provider management, session fork, elicitation (draft ops).
- `unstable_mcp_over_acp` — code-generates `mcp/connect`, `mcp/message`,
  `mcp/disconnect` relay methods. Without it, MCP relay types don't exist.

## Documents in this guide

| # | File | Covers |
|---|------|--------|
| 1 | [01-getting-started.md](01-getting-started.md) | Cargo deps, transport (`AcpAgent` vs `ByteStreams`), process lifecycle, the `Client.builder()` + handler + `connect_with` dispatch-loop model, non-blocking callback spawning |
| 2 | [02-session-lifecycle.md](02-session-lifecycle.md) | `initialize` (clientInfo, clientCapabilities), capability caching, `session/new` vs `session/load` vs `session/list` resolution, `SessionId`, MCP server attachment |
| 3 | [03-prompts-streaming-cancellation.md](03-prompts-streaming-cancellation.md) | `PromptRequest` construction (`ContentBlock`, `EmbeddedResource`), `send_request` + `block_task` + `tokio::select!`, `SessionNotification`/`SessionUpdate` translation, `PromptResponse.stop_reason`, `CancelNotification` |
| 4 | [04-client-handlers.md](04-client-handlers.md) | `fs/read_text_file`, `fs/write_text_file` (path containment), `session/request_permission` (option mapping, response shape), `terminal/*` family (create/output/wait/kill/release, env filtering, output bounding) |
| 5 | [05-providers-and-config.md](05-providers-and-config.md) | Hand-rolled `JsonRpcRequest` derive for `providers/*`, `session/set_config_option` for model & profile, config-option discovery helpers |
| 6 | [06-type-reference.md](06-type-reference.md) | Full tables of request/response/notification types, builder patterns, key enums |
| 7 | [07-gotchas.md](07-gotchas.md) | `futures::io` vs tokio, `async_process` vs `tokio::process`, `block_task` to avoid dispatch deadlock, non-exhaustive enums, Windows path escaping, MCP v1 dispatch caveat |

## How to read this guide

Read **01** first — it establishes the mental model (builder + handler +
dispatch loop) that every other document builds on. Then read in order or jump
to the topic you need. **06** is a reference to consult as you write code.

All code snippets are real, verified shapes — not pseudocode. Copy them as
starting points and adapt the dependency-injection (`HandlerDeps`-equivalent)
to your app.
