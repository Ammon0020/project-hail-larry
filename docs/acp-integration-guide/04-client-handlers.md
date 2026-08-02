# 04 — Client-Side Handlers: Filesystem, Permission, Terminal

The agent sends **requests** to your client for filesystem access, permission
decisions, and terminal (shell) operations. You register one handler per
request type via `.on_receive_request(...)`. This document covers each family.

**Recall from [01-getting-started.md](01-getting-started.md):** every handler
must **spawn long-running work and return `Ok(())` immediately** so the dispatch
loop isn't blocked. Use the `spawn_result_callback` / `spawn_respond_callback`
helpers from §"Non-blocking callbacks".

## Filesystem: `fs/read_text_file` & `fs/write_text_file`

The agent asks you to read/write files. **You own the filesystem** — validate
that requested paths stay inside the workspace before touching disk.

### Path containment (security-critical)

Agents can request absolute paths. Strip the workspace root and reject anything
outside it:

```rust
use std::path::Path;

fn workspace_relative_path(root: &Path, path: &Path) -> Result<String, MyError> {
    if path.is_absolute() {
        let relative = path.strip_prefix(root)
            .map_err(|_| MyError::validation("agent path is outside the workspace"))?;
        Ok(relative.to_string_lossy().into_owned())
    } else {
        Ok(path.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_path_escape() {
        #[cfg(unix)]
        let (root, outside) = (Path::new("/workspace"), Path::new("/outside/file"));
        #[cfg(windows)]
        let (root, outside) = (Path::new(r"C:\workspace"), Path::new(r"D:\outside\file"));
        assert!(workspace_relative_path(root, outside).is_err());
    }
}
```

**Also reject symlinks** that escape the workspace (per ACP security guidance).
Your workspace manager should canonicalize and verify containment on every
read/write.

### `ReadTextFileRequest` handler

```rust
use agent_client_protocol::schema::v1::{
    ReadTextFileRequest, ReadTextFileResponse,
};

async fn read_text_file(deps: HandlerDeps, request: ReadTextFileRequest) -> Result<ReadTextFileResponse, MyError> {
    let path = workspace_relative_path(&deps.workspace_path, &request.path)?;
    let result = deps.workspaces.read_file(&deps.workspace_id, &path).await?;
    Ok(ReadTextFileResponse::new(result.content))
}

// Registration:
.on_receive_request(
    {
        let deps = handler_deps.clone();
        async move |request: ReadTextFileRequest, responder, _cx: ConnectionTo<Agent>| {
            spawn_result_callback(
                deps.clone(), responder, "ACP denied file read",
                move |deps| async move { read_text_file(deps, request).await },
            );
            Ok(())
        }
    },
    agent_client_protocol::on_receive_request!(),
)
```

### `WriteTextFileRequest` handler

```rust
use agent_client_protocol::schema::v1::{WriteTextFileRequest, WriteTextFileResponse};

async fn write_text_file(deps: HandlerDeps, request: WriteTextFileRequest) -> Result<WriteTextFileResponse, MyError> {
    let path = workspace_relative_path(&deps.workspace_path, &request.path)?;
    deps.workspaces.write_file(&deps.workspace_id, &path, &request.content, 0).await?;

    // Broadcast a "file written" event so your UI refreshes the file tree.
    // If the event bus fails, the file is already on disk — log loudly but
    // DON'T fail the ACP response (that would mislead the agent).
    if let Err(error) = deps.event_bus.publish(FileWritten { workspace_id: deps.workspace_id.clone(), target: path }).await {
        tracing::error!(%error, "failed to publish FileWritten after agent write");
    }
    Ok(WriteTextFileResponse::new())
}
```

**Why not fail on event-bus error?** The file is already written. Returning an
error would make the agent think the write failed and possibly retry or report
a false failure. Log loudly so a broken bus is visible, but return success.

## Permission: `session/request_permission`

Before executing a tool call (especially shell commands), the agent sends a
permission request with a list of options (allow once, allow always, reject
once, reject always). You surface these to the user and respond with their
choice.

### Request shape

```rust
// agent_client_protocol::schema::v1::RequestPermissionRequest
// {
//     session_id: SessionId,
//     tool_call: ToolCallUpdate {          // yes, the UPDATE type — see note below
//         tool_call_id: ToolCallId,
//         fields: ToolCallUpdateFields {   // #[serde(flatten)] — all fields optional
//             title: Option<String>,
//             kind: Option<ToolKind>,
//             status: Option<ToolCallStatus>,
//             raw_input: Option<serde_json::Value>,  // command text, etc.
//             raw_output: Option<serde_json::Value>,
//             locations: Option<Vec<ToolCallLocation>>, // file paths
//             content: Option<Vec<ToolCallContent>>,
//         },
//     },
//     options: Vec<PermissionOption>, // each has option_id, name, kind
// }
//
// PermissionOption {
//     option_id: PermissionOptionId,  // newtype around Arc<str> (like SessionId)
//     name: String,
//     kind: PermissionOptionKind,     // AllowOnce | AllowAlways | RejectOnce | RejectAlways
// }
```

> **`ToolCall` vs `ToolCallUpdate` — important distinction.**
> ACP has two tool-call shapes that share field names but differ in optionality:
> - **`ToolCall`** (carried by `SessionUpdate::ToolCall`) has **direct, non-optional
>   fields**: `title: String`, `kind: ToolKind`, `status: ToolCallStatus`,
>   `locations: Vec<ToolCallLocation>`, `raw_input: Option<serde_json::Value>`.
>   Access as `tool_call.title`, `tool_call.kind`, etc. (no `.fields`).
> - **`ToolCallUpdate`** (carried by `SessionUpdate::ToolCallUpdate` **and** by
>   `RequestPermissionRequest.tool_call`) has a **flattened `fields:
>   ToolCallUpdateFields`** where **every field is `Option<...>`**:
>   `fields.title: Option<String>`, `fields.kind: Option<ToolKind>`, etc.
>   Access as `update.fields.title`, `update.fields.kind`, etc.
>
> The permission request reuses the **update** shape (all fields optional) because
> the agent may omit fields it doesn't consider relevant to the permission
> decision. Always use `.fields` and handle `Option` when reading a permission
> request's tool call.

### Response shape

```rust
use agent_client_protocol::schema::v1::{
    RequestPermissionOutcome, RequestPermissionResponse, SelectedPermissionOutcome,
};

// User allowed → echo the chosen option's id:
RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
    SelectedPermissionOutcome::new(option_id), // PermissionOptionId
));

// User rejected / timed out / error:
RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled);
```

### Handler (uses `spawn_respond_callback` — errors become `Cancelled`, not internal errors)

Permission waits for a human on another device, so it can take a long time.
**Map failures to `Cancelled`** (not JSON-RPC internal error) — the agent treats
a cancelled permission gracefully but an internal error may abort the turn.

```rust
use agent_client_protocol::schema::v1::{
    PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome,
};

async fn request_permission(deps: HandlerDeps, request: RequestPermissionRequest) -> RequestPermissionResponse {
    let tool = request.tool_call.fields.title.clone().unwrap_or_else(|| "Tool call".into());
    let tool_kind = request.tool_call.fields.kind.map(tool_kind_name).unwrap_or_default();
    let command = request.tool_call.fields.raw_input.as_ref()
        .map_or(String::new, ToString::to_string);
    let target = request.tool_call.fields.locations.as_ref()
        .and_then(|locs| locs.first())
        .map_or(String::new, |l| l.path.to_string_lossy().into_owned());

    // Map ACP option kinds to your app's decision enum.
    let options: Vec<PermissionDecision> = request.options.iter()
        .filter_map(|o| permission_decision(o.kind)).collect();

    let permission = PermissionRequest {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: deps.local_session_id.clone(), // YOUR session id, not the agent's
        tool, tool_kind, command, target, options,
    };

    match deps.permissions.request(permission).await {
        Ok(decision) => {
            // Find the ACP option matching the user's decision and echo its id.
            request.options.iter()
                .find(|o| permission_decision(o.kind) == Some(decision))
                .map_or(
                    || RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled),
                    |o| RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
                        SelectedPermissionOutcome::new(o.option_id.clone()),
                    )),
                )
        }
        Err(error) => {
            tracing::warn!(%error, "permission request cancelled");
            RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
        }
    }
}

fn permission_decision(kind: PermissionOptionKind) -> Option<PermissionDecision> {
    match kind {
        PermissionOptionKind::AllowOnce => Some(PermissionDecision::AllowOnce),
        PermissionOptionKind::AllowAlways => Some(PermissionDecision::AllowAlways),
        PermissionOptionKind::RejectOnce => Some(PermissionDecision::Deny),
        PermissionOptionKind::RejectAlways => Some(PermissionDecision::RejectAlways),
        _ => None, // non-exhaustive
    }
}

fn permission_kind_name(kind: PermissionOptionKind) -> &'static str {
    match kind {
        PermissionOptionKind::AllowOnce => "allow_once",
        PermissionOptionKind::AllowAlways => "allow_always",
        PermissionOptionKind::RejectOnce => "reject_once",
        PermissionOptionKind::RejectAlways => "reject_always",
        _ => "unknown",
    }
}
```

### Client-synthesized "Always allow this tool type" option (optional)

ACP has no `AllowToolKind` option kind. If you want a per-tool-kind policy
("always allow file reads"), **synthesize a client-only option** and append it
to both the options list (so the user can pick it) and the response mapping.
When the user picks it, respond to the agent with `AllowAlways` (the broadest
ACP allow) so the agent proceeds.

**Be conservative:** only synthesize this for non-execute tool kinds (`move`,
`edit`, `read`, `search`). A blanket "always allow shell commands" for `execute`
would be a security risk — one approval for `echo hello` would auto-approve
`rm -rf /`.

```rust
const TOOL_KIND_ALLOWLIST: &[&str] = &["move", "edit", "read", "search"];

if TOOL_KIND_ALLOWLIST.contains(&tool_kind.as_str()) {
    options.push(PermissionDecision::AllowToolKind);
    // ... append to option_details for UI rendering ...
}

// On AllowToolKind decision: find the AllowAlways option and echo its id.
if decision == PermissionDecision::AllowToolKind {
    if let Some(option) = request.options.iter()
        .find(|o| permission_decision(o.kind) == Some(PermissionDecision::AllowAlways))
    {
        return RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(option.option_id.clone()),
        ));
    }
}
```

### Registration

```rust
.on_receive_request(
    {
        let deps = handler_deps.clone();
        async move |request: RequestPermissionRequest, responder, _cx: ConnectionTo<Agent>| {
            // spawn_respond_callback (not spawn_result_callback): the work
            // always returns a typed success value (Cancelled on error).
            spawn_respond_callback(deps.clone(), responder, move |deps| async move {
                request_permission(deps, request).await
            });
            Ok(())
        }
    },
    agent_client_protocol::on_receive_request!(),
)
```

## Terminal: `terminal/*` family (shell execution)

ACP has **no `ExecuteCommand` request**. Shell execution is delegated to the
`terminal/*` family: create a terminal, read its output, wait for exit, kill,
release. You register handlers for all five.

### `terminal/create` — start a command

```rust
use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, CreateTerminalResponse, TerminalExitStatus,
};

async fn create_terminal(deps: HandlerDeps, request: CreateTerminalRequest) -> Result<CreateTerminalResponse, MyError> {
    if deps.cancellation.is_cancelled() {
        return Err(MyError::internal("session is closing"));
    }
    let cwd = terminal_cwd(&deps.workspace_path, request.cwd.as_deref())?;
    let limit = request.output_byte_limit
        .map_or(MAX_OUTPUT_BYTES, |l| usize::try_from(l).unwrap_or(MAX_OUTPUT_BYTES).min(MAX_OUTPUT_BYTES));

    let terminal_id = format!("term-{}", uuid::Uuid::new_v4().simple());
    let state = Arc::new(TerminalState::new(limit, deps.cancellation.child_token()));
    deps.terminals.lock().unwrap().insert(terminal_id.clone(), Arc::clone(&state));

    // SECURITY: filter env vars.
    // - Strip dangerous daemon secrets (API keys, etc.) from the inherited env.
    // - Strip hijack vars (LD_PRELOAD, DYLD_*) from the agent-supplied env.
    // - Merge the filtered daemon env with the filtered agent env.
    let env = merge_env(
        filter_daemon_env(std::env::vars()),
        filter_agent_env(request.env.iter().map(|v| (v.name.clone(), v.value.clone()))),
    );

    let command = request.command;
    let args = request.args;
    let workspace = deps.workspace_path.clone();
    tokio::spawn(async move {
        let executor = Executor::new(&workspace).with_env(env).with_max_output_bytes(limit);
        let (result, error) = executor.run_async(
            state.cancel.clone(), &command, &args, cwd.as_deref(),
            |line| state.push_line(line),  // stdout
            |line| state.push_line(line),  // stderr
        ).await;
        if let Some(error) = error {
            // Log the error CATEGORY only — commands/argv/env may contain credentials.
            tracing::warn!(%error, "terminal command ended abnormally");
        }
        #[allow(clippy::cast_sign_loss)]
        let exit_code = (result.exit_code >= 0).then_some(result.exit_code as u32);
        let status = TerminalExitStatus::new().exit_code(exit_code).signal(result.signal);
        state.set_exit(status);
    });

    Ok(CreateTerminalResponse::new(terminal_id))
}
```

**Permission note:** `terminal/create` itself carries no permission
precondition. Per ACP spec, `session/request_permission` is **agent-initiated
(MAY)** — the agent asks for permission *before* calling `terminal/create`. Your
client must respond to the permission request but must **not** independently
re-gate `terminal/create`. The harness owns the permission decision; you just
execute the approved command.

### `terminal/output` — snapshot current output (non-blocking)

```rust
use agent_client_protocol::schema::v1::{TerminalOutputRequest, TerminalOutputResponse};

fn terminal_output(deps: &HandlerDeps, request: &TerminalOutputRequest) -> Result<TerminalOutputResponse, MyError> {
    let terminal = terminal_state(&deps.terminals, &request.terminal_id.to_string())?;
    let output = terminal.output.lock().unwrap();
    let exit_status = terminal.exit.borrow().clone();
    Ok(TerminalOutputResponse::new(output.text.clone(), output.truncated).exit_status(exit_status))
}
```

### `terminal/wait_for_exit` — block until the command exits

```rust
use agent_client_protocol::schema::v1::{WaitForTerminalExitRequest, WaitForTerminalExitResponse};

async fn wait_for_terminal_exit(deps: HandlerDeps, request: WaitForTerminalExitRequest) -> Result<WaitForTerminalExitResponse, MyError> {
    let terminal = terminal_state(&deps.terminals, &request.terminal_id.to_string())?;
    let mut exit = terminal.exit.subscribe();
    loop {
        if let Some(status) = exit.borrow().clone() {
            return Ok(WaitForTerminalExitResponse::new(status));
        }
        exit.changed().await
            .map_err(|_| MyError::internal("terminal exited without a status"))?;
    }
}
```

Use a `tokio::sync::watch` channel to signal exit: the spawned command task
sends `Some(status)` via `watch::Sender::send_replace`; `wait_for_exit`
subscribes and loops on `changed().await`. **This handler must be spawned**
(via `spawn_result_callback`) because it can block indefinitely.

### `terminal/kill` & `terminal/release`

```rust
use agent_client_protocol::schema::v1::{
    KillTerminalRequest, KillTerminalResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
};

fn kill_terminal(deps: &HandlerDeps, request: &KillTerminalRequest) -> Result<KillTerminalResponse, MyError> {
    let terminal = terminal_state(&deps.terminals, &request.terminal_id.to_string())?;
    terminal.cancel.cancel(); // CancellationToken — aborts the spawned command
    Ok(KillTerminalResponse::new())
}

fn release_terminal(deps: &HandlerDeps, request: &ReleaseTerminalRequest) -> Result<ReleaseTerminalResponse, MyError> {
    deps.terminals.lock().unwrap()
        .remove(&request.terminal_id.to_string())
        .ok_or_else(|| MyError::not_found("terminal"))?;
    Ok(ReleaseTerminalResponse::new())
}
```

`kill` cancels but **retains** the terminal (output still readable). `release`
cancels **and removes** it from the registry.

### Bounded output (memory safety)

Terminals can produce unbounded output. Cap retained output and discard the
oldest complete UTF-8 prefix when over the limit:

```rust
struct RetainedOutput { text: String, limit: usize, truncated: bool }

impl RetainedOutput {
    fn push_line(&mut self, line: &str) {
        if self.limit == 0 { self.truncated = true; return; }
        self.text.push_str(line);
        self.text.push('\n');
        if self.text.len() > self.limit {
            let excess = self.text.len() - self.limit;
            let start = self.text.ceil_char_boundary(excess); // don't split a UTF-8 char
            self.text.drain(..start);
            self.truncated = true;
        }
    }
}
```

### Terminal registry & capacity

Cap terminals per session (e.g. 16) to prevent resource exhaustion:

```rust
const MAX_TERMINALS_PER_SESSION: usize = 16;

let mut terminals = deps.terminals.lock().unwrap();
if terminals.len() >= MAX_TERMINALS_PER_SESSION {
    return Err(MyError::internal("terminal capacity exceeded"));
}
terminals.insert(terminal_id.clone(), state);
```

### Session-close cleanup

When the session closes, cancel all live terminals:

```rust
fn cancel_terminals(registry: &TerminalRegistry) {
    if let Ok(terminals) = registry.lock() {
        for state in terminals.values() {
            state.cancel.cancel();
        }
    }
}
// Call this after connect_with returns, alongside handler_cancel.cancel().
```

## Registration summary

Register all five terminal handlers (plus fs + permission) before
`connect_with`. Each follows the same spawn-and-return pattern:

```rust
Client.builder()
    .name("my-app")
    .on_receive_request(/* ReadTextFileRequest */, on_receive_request!())
    .on_receive_request(/* WriteTextFileRequest */, on_receive_request!())
    .on_receive_request(/* RequestPermissionRequest */, on_receive_request!())
    .on_receive_request(/* CreateTerminalRequest */, on_receive_request!())
    .on_receive_request(/* TerminalOutputRequest */, on_receive_request!())
    .on_receive_request(/* WaitForTerminalExitRequest */, on_receive_request!())
    .on_receive_request(/* KillTerminalRequest */, on_receive_request!())
    .on_receive_request(/* ReleaseTerminalRequest */, on_receive_request!())
    .on_receive_notification(/* SessionNotification */, on_receive_notification!())
    .connect_with(transport, main_fn)
    .await?;
```

**Only register handlers for capabilities you advertised in `initialize`'s
`client_capabilities`.** If you set `fs.read_text_file(true)`, you must register
the `ReadTextFileRequest` handler.
