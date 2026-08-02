# 02 — Session Lifecycle: Initialize, New/Load, Capabilities

This document covers what goes inside `main_fn` (the closure passed to
`connect_with`) up to the point where you have a live session ready to prompt:
the `initialize` handshake, capability caching, and `session/new` vs
`session/load` resolution.

## The startup sequence

```
connect_with(transport, |cx| async {
    1. send initialize request          → cache capabilities
    2. (optional) load MCP servers      → filter by agent caps
    3. resolve session: new or load     → get agent SessionId
    4. (optional) set initial config    → model/profile
    5. run the prompt/control loop      → see 03-prompts...
    6. teardown (return)                → SDK drops transport, kills child
})
```

## Step 1: `initialize`

`initialize` is the first request you send. It negotiates the protocol version
and advertises your client capabilities (which filesystem/terminal ops you
support). The response tells you what the **agent** supports.

```rust
use agent_client_protocol::schema::v1::{
    ClientCapabilities, FileSystemCapabilities, Implementation, InitializeRequest,
    InitializeResponse,
};
use agent_client_protocol::schema::ProtocolVersion;

let initialize = InitializeRequest::new(ProtocolVersion::V1)
    .client_info(Implementation::new("my-app", "1.0"))   // MUST be non-empty
    .client_capabilities(
        ClientCapabilities::new()
            .fs(
                FileSystemCapabilities::new()
                    .read_text_file(true)
                    .write_text_file(true),
            )
            .terminal(true),
    );

let init: InitializeResponse = cx
    .send_request(initialize)
    .block_task()
    .await?;
```

### `clientInfo` — non-empty requirement

`client_info.name` and `.version` **must be non-empty**. Some agents (e.g.
Mistral Vibe) forward these into upstream provider metadata that rejects blank
values. Don't use empty strings or defaults that serialize to `""`.

### `clientCapabilities` — advertise what you implement

Only advertise capabilities you actually register handlers for. If you set
`fs.read_text_file(true)`, the agent will send you `fs/read_text_file`
requests — your handler must exist and respond. Mismatched caps + missing
handlers cause the agent to hang or error.

| Capability | Set true when you register... |
|------------|-------------------------------|
| `fs.read_text_file` | `ReadTextFileRequest` handler |
| `fs.write_text_file` | `WriteTextFileRequest` handler |
| `terminal` | `CreateTerminalRequest` + `TerminalOutputRequest` + `WaitForTerminalExitRequest` + `KillTerminalRequest` + `ReleaseTerminalRequest` handlers |

### Caching `InitializeResponse` capabilities

The response's `agent_capabilities` field tells you what the agent can do. Cache
the relevant booleans so later RPCs can gate without re-probing:

```rust
use agent_client_protocol::schema::v1::InitializeResponse;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionCaps {
    pub providers_supported: bool,    // agent_capabilities.providers.is_some()
    pub embedded_context: bool,       // prompt_capabilities.embedded_context
    pub can_list_sessions: bool,      // session_capabilities.list.is_some()
    pub can_load_session: bool,       // agent_capabilities.load_session
    pub can_resume_session: bool,     // session_capabilities.resume.is_some()
    pub can_close_session: bool,      // session_capabilities.close.is_some()
    pub can_delete_session: bool,     // session_capabilities.delete.is_some()
}

fn caps_from_init(init: &InitializeResponse) -> SessionCaps {
    let agent_caps = &init.agent_capabilities;
    let session_caps = &agent_caps.session_capabilities;
    SessionCaps {
        providers_supported: agent_caps.providers.is_some(),
        embedded_context: agent_caps.prompt_capabilities.embedded_context,
        can_list_sessions: session_caps.list.is_some(),
        can_load_session: agent_caps.load_session,
        can_resume_session: session_caps.resume.is_some(),
        can_close_session: session_caps.close.is_some(),
        can_delete_session: session_caps.delete.is_some(),
    }
}
```

These gates matter: sending `session/load` to an agent that didn't advertise
`loadSession` will error; sending `providers/list` without `providers` caps
returns 501-class unsupported.

## Step 2: (optional) load MCP servers

If your app uses MCP (Model Context Protocol) servers, load and filter them
**before** `session/new`, then attach via the request's `.mcp_servers(...)` field.
MCP is **additive**: malformed/missing config must never block session creation.

```rust
use agent_client_protocol::schema::v1::{McpCapabilities, McpServer};

/// Load enabled MCP servers, filtered by agent capabilities.
/// Missing path / parse errors yield an empty list (never fail session create).
async fn load_mcp_servers(
    mcp_config_path: Option<&Path>,
    caps: &McpCapabilities,
) -> Vec<McpServer> {
    let Some(path) = mcp_config_path else { return Vec::new() };
    match parse_mcp_config(path) {
        Ok(servers) => filter_by_capabilities(servers, caps),
        Err(error) => {
            tracing::warn!(%error, "mcp config load failed; continuing without mcp");
            Vec::new()
        }
    }
}

let mcp_servers = load_mcp_servers(mcp_config_path, &init.agent_capabilities.mcp_capabilities).await;
```

`McpCapabilities::new()` defaults to stdio-only. Set `.http(true)` /
`.sse(true)` if the agent advertises those transports. `McpServer::Stdio(...)`
is the common variant; `McpServer::Http(...)` / `McpServer::Sse(...)` require
the matching capability.

**Feature requirement:** MCP relay types (`ConnectMcpRequest`, etc.) only exist
when the `unstable_mcp_over_acp` feature is enabled. See
[07-gotchas.md](07-gotchas.md) §"MCP v1 dispatch caveat" for the v1 enum-dispatch
pattern.

## Step 3: resolve the session — `session/new` vs `session/load`

ACP sessions have two identities:
- **Your local session id** — your app's identifier (UUID, etc.).
- **The agent's ACP session id** — returned by `session/new` or `session/load`.
  Persist this so you can resume after a restart.

Resolution logic (mirrors the production `resolve_acp_session`):

```
1. If you have a persisted ACP session id AND the agent supports load:
   a. If the agent supports session/list: call session/list first.
      - If the persisted id is NOT in the list → create new (it's gone).
      - If list fails → fall through to try load anyway.
   b. Call session/load with the persisted id.
      - Success → resume the persisted session.
      - Failure → fall back to session/new.
2. Else → session/new.
```

```rust
use agent_client_protocol::schema::v1::{
    ListSessionsRequest, LoadSessionRequest, NewSessionRequest, SessionConfigOption,
    SessionId,
};

async fn resolve_session(
    cx: &ConnectionTo<Agent>,
    init: &InitializeResponse,
    workspace_path: &Path,
    mcp_servers: Vec<McpServer>,
    persisted_id: &str,  // empty string = always create new
) -> Result<(SessionId, Option<Vec<SessionConfigOption>>), agent_client_protocol::Error> {
    let can_load = init.agent_capabilities.load_session;
    let can_list = init.agent_capabilities.session_capabilities.list.is_some();
    let should_load = !persisted_id.is_empty() && can_load;

    // Reconcile via session/list when supported.
    if should_load && can_list {
        match cx.send_request(ListSessionsRequest::new().cwd(workspace_path.to_path_buf()))
            .block_task().await
        {
            Ok(listed) => {
                if !listed.sessions.iter().any(|s| s.session_id.to_string() == persisted_id) {
                    return new_session(cx, workspace_path, mcp_servers).await;
                }
                // Confirmed present — attempt load below.
            }
            Err(error) => {
                tracing::info!(%error, "session/list failed; falling through to load");
            }
        }
    }

    if should_load {
        let load_req = LoadSessionRequest::new(SessionId::new(persisted_id), workspace_path)
            .mcp_servers(mcp_servers.clone());
        match cx.send_request(load_req).block_task().await {
            Ok(loaded) => {
                return Ok((SessionId::new(persisted_id), loaded.config_options));
            }
            Err(error) => {
                tracing::info!(%error, "session/load failed; falling back to new");
            }
        }
    }

    new_session(cx, workspace_path, mcp_servers).await
}

async fn new_session(
    cx: &ConnectionTo<Agent>,
    workspace_path: &Path,
    mcp_servers: Vec<McpServer>,
) -> Result<(SessionId, Option<Vec<SessionConfigOption>>), agent_client_protocol::Error> {
    let session = cx
        .send_request(NewSessionRequest::new(workspace_path).mcp_servers(mcp_servers))
        .block_task()
        .await?;
    Ok((session.session_id, session.config_options))
}
```

### `SessionId` — the newtype

`SessionId` is a `#[serde(transparent)]` newtype around `Arc<str>`:

```rust
// Construct:
let id = SessionId::new("acp-123");        // from &str
let id = SessionId::new(String::from("...")); // from String
let id = SessionId::new(Arc::<str>::from("...")); // from Arc<str>

// Read:
let s: String = id.to_string();
let inner: &str = &id.0; // access the inner Arc<str>
```

It implements `Display`, `Clone` (cheap — `Arc`), `Eq`/`Hash`. Pass it by value
(`.clone()`) into requests; it's cheap.

### `config_options` — what the agent advertises

Both `NewSessionResponse` and `LoadSessionResponse` carry
`config_options: Option<Vec<SessionConfigOption>>`. These are the agent's
selectable config knobs (model choice, mode/profile). Inspect them to find the
**model config option id** and **profile config option id** so you can later
switch model/profile via `session/set_config_option`. See
[05-providers-and-config.md](05-providers-and-config.md) §"Config-option discovery".

## Step 4: (optional) set initial config

If the agent advertised a profile/mode config option, send the initial selection
**best-effort** (failure is logged, not fatal — profile is a hint):

```rust
if let Some(config_id) = profile_config_id {
    if let Err(error) = set_profile_config(&cx, &agent_session_id, &config_id, &active_profile).await {
        tracing::warn!(%error, "initial profile set failed; agent keeps its config");
    }
}
```

## Step 5+: prompt loop & teardown

See [03-prompts-streaming-cancellation.md](03-prompts-streaming-cancellation.md)
for the prompt/control loop. Teardown happens when `main_fn` returns: the SDK
drops the transport, which kills the agent child (see
[01-getting-started.md](01-getting-started.md) §"Process-group isolation").

## Auth note

`InitializeResponse.auth_methods: Vec<AuthMethod>` may be non-empty. If the agent
requires auth, it will reject `session/new` with an `auth_required` error. The
flow is:

1. `InitializeResponse.auth_methods` lists methods (e.g. `Agent` default,
   `EnvVar`/`Terminal` behind `unstable_auth_methods`).
2. Send `AuthenticateRequest::new(method_id)` → `AuthenticateResponse`.
3. On success, retry `session/new`.

A real OAuth/PKCE dance requires a real agent. Detect auth-required failures and
surface a clear "run `<agent> login` on the host" message to the operator.

```rust
// After session/new fails:
if error.to_string().to_ascii_lowercase().contains("authentication") {
    tracing::error!(
        "AGENT AUTHENTICATION REQUIRED: run `{} login` on the host to authenticate.",
        agent_command
    );
}
```
