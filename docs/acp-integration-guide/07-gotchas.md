# 07 — Gotchas & Pitfalls

Hard-won lessons from production usage. Read this before you start and again
when something doesn't compile or hangs.

## 1. `futures::io` vs `tokio::io` — use `async_process`, not `tokio::process`

The SDK's transport layer uses **`futures::io::{AsyncRead, AsyncWrite}`** (the
`futures-io` crate traits), NOT tokio's `AsyncRead`/`AsyncWrite`.

- ✅ `async_process::Command` / `async_process::ChildStdin` / `ChildStdout` —
  these implement `futures::io` traits. Use with `ByteStreams::new(stdin, stdout)`.
- ❌ `tokio::process::Command` / `tokio::process::ChildStdin` — these implement
  tokio's traits, NOT `futures::io`. They won't compile with `ByteStreams`.

If you need tokio for everything else but `async_process` for the agent child,
that's fine — they coexist. Just don't mix the stream types.

## 2. `block_task()` to avoid dispatch-loop deadlock

`cx.send_request(req)` returns a `SentRequest<T>`. If you `.await` it directly
inside `main_fn` (the `connect_with` closure), you **deadlock the dispatch
loop** — the loop can't read the response because it's blocked on your await.

**Always use `.block_task().await`**, which spawns a task to await the response
and returns the result to you without blocking dispatch.

```rust
// ✅ Correct:
let resp = cx.send_request(req).block_task().await?;

// ❌ Deadlock (awaiting SentRequest directly):
let resp = cx.send_request(req).await?; // may not even compile, but conceptually wrong
```

To select over a pending request (e.g., race prompt vs cancel), pin it first:

```rust
let prompt = cx.send_request(req).block_task();
tokio::pin!(prompt);
tokio::select! {
    reply = &mut prompt => { /* ... */ }
    cmd = rx.recv() => { /* ... */ }
}
```

## 3. Never block the dispatch loop in a handler

Handlers run on the dispatch loop task. A handler that `await`s a long
operation (user permission prompt, terminal command) blocks **all** other
request/notification dispatch — including the streaming updates your UI needs.

**Spawn long-running handler work and return `Ok(())` immediately.** Use a
bounded `Semaphore` + `CancellationToken` (see
[01-getting-started.md](01-getting-started.md) §"Non-blocking callbacks").

```rust
// ✅ Correct:
async move |request, responder, _cx| {
    spawn_result_callback(deps.clone(), responder, move |deps| async move {
        do_work(deps, request).await
    });
    Ok(()) // return immediately
}

// ❌ Blocks dispatch:
async move |request, responder, _cx| {
    let result = do_work(deps, request).await; // blocks!
    responder.respond(result);
    Ok(())
}
```

## 4. `clientInfo` must be non-empty

`InitializeRequest.client_info.name` and `.version` **must be non-empty
strings**. Some agents (e.g. Mistral Vibe) forward these into upstream provider
metadata that rejects blank values. Don't use `""` or rely on a default that
serializes to empty.

```rust
InitializeRequest::new(ProtocolVersion::V1)
    .client_info(Implementation::new("my-app", "1.0")) // both non-empty
```

## 5. Non-exhaustive enums — always include a fallback

**Every** schema enum (`StopReason`, `ToolKind`, `ToolCallStatus`,
`PermissionOptionKind`, `SessionUpdate`, `ContentBlock`, `ErrorCode`, etc.) is
`#[non_exhaustive]`. A `match` without a `_` arm will fail to compile, and
future protocol versions add variants you don't know about.

```rust
// ✅ Always:
match reason {
    StopReason::EndTurn => "end_turn",
    // ...
    _ => "unknown",
}

// ❌ Won't compile (non-exhaustive):
match reason {
    StopReason::EndTurn => "end_turn",
    StopReason::MaxTokens => "max_tokens",
    // missing variants → compile error
}
```

This also protects against silent data loss on protocol upgrades: an unknown
variant hits your fallback instead of panicking.

## 6. Windows: `AcpAgent::from_str` mangles backslash paths

`AcpAgent::from_str` parses with `shell_words::split`, which treats `\` as an
escape. Windows paths like `C:\tmp\agent.exe` get mangled. On Windows, pass the
command as JSON to bypass the shell-words parser:

```rust
#[cfg(windows)]
{
    let json = format!(
        r#"{{"type":"stdio","name":"agent","command":{},"args":[],"env":[]}}"#,
        serde_json::to_string(&bin_path)?
    );
    AcpAgent::from_str(&json)?
}
#[cfg(not(windows))]
{
    AcpAgent::from_str(&bin_path)?
}
```

## 7. Process tree teardown — `kill_on_drop` isn't enough

`kill_on_drop(true)` on the `async_process::Child` only kills the **direct
child**. Agents spawn descendants (language servers, build tools, shells) that
survive as orphans. On Unix, put the child in its own process group before
exec, then kill the **group** on teardown:

```rust
// Before spawn: setpgid(0, 0) via pre_exec
let mut std_cmd = std::process::Command::new(&binary);
configure_process_group(&mut std_cmd); // your helper: std::os::unix::process::CommandExt::pre_exec
let mut command = async_process::Command::from(std_cmd);
command.kill_on_drop(true);
let mut child = command.spawn()?;

// After connect_with returns (teardown):
#[cfg(unix)]
if let Ok(pid) = i32::try_from(child.id()) {
    // kill(-pgid, SIGKILL) kills the whole group
    unsafe { libc::kill(-pid, libc::SIGKILL); }
}
let _ = child.kill();
let _ = child.status().await; // reap to avoid zombie
```

Keep a `ProcessGroupCleanup` guard alive until after the reap so an early
return / panic still kills the group.

## 8. Dropping `SentRequest` auto-sends `$/cancel_request`

This is the **teardown guarantee**, but it can surprise you:

- If you let a `SentRequest` go out of scope without `detach()` or
  `block_task()`, the SDK sends `$/cancel_request` to the agent.
- This is usually what you want on teardown, but if you accidentally drop a
  request you meant to keep alive, you'll cancel it.
- Use `.detach()` to keep the request running on the peer while ignoring the
  response.

## 9. MCP v1 dispatch caveat

With the `unstable_mcp_over_acp` feature, the standalone MCP types
(`ConnectMcpRequest`, `MessageMcpRequest`, etc.) **do not implement
`JsonRpcMessage` directly for protocol v1** — they're only reachable as
variants of the `AgentRequest` / `AgentResponse` / `AgentNotification` enums.

Send via the enum:
```rust
cx.send_request::<AgentRequest>(AgentRequest::ConnectMcpRequest(req))
// match the AgentResponse enum on the response
```

And register a single `on_receive_request::<AgentRequest>` handler that
dispatches all agent→client requests in one closure. (Standalone
`JsonRpcMessage` impls exist only for the v2 draft.)

## 10. Handler state capture — the double-clone pattern

Handlers are `FnMut`. To share `Arc` state across invocations, you need the
double-clone pattern:

```rust
.on_receive_request(
    {
        let deps = deps.clone(); // (1) clone into the closure's captured env
        async move |request, responder, _cx| {
            let deps = deps.clone(); // (2) re-clone per invocation (FnMut, not FnOnce)
            // ... use deps ...
            Ok(())
        }
    },
    agent_client_protocol::on_receive_request!(),
)
```

Without the inner clone (2), the closure would move `deps` on the first call
and fail on the second (`FnOnce` instead of `FnMut`).

## 11. Permission errors → `Cancelled`, not internal error

`session/request_permission` waits for a human. Failures (timeout, user
dismissed, error) should map to `RequestPermissionOutcome::Cancelled`, **not**
a JSON-RPC internal error. The agent handles `Cancelled` gracefully (treats the
tool as rejected); an internal error may abort the whole turn.

Use `spawn_respond_callback` (always returns a typed success value) for
permission, not `spawn_result_callback` (which maps errors to internal errors).

## 12. Don't fail `fs/write_text_file` on event-bus errors

After a successful file write, if your event-bus publish fails (e.g., SQLite
error), **don't fail the ACP response**. The file is already on disk; returning
an error would make the agent think the write failed and retry or report a
false failure. Log loudly, return success.

## 13. Log error codes, not error text

`agent_client_protocol::Error` may carry agent-controlled text (prompts,
command args, credentials). When logging errors that flow into persisted/synced
events, derive a **safe label from `error.code`** instead of logging
`%error`:

```rust
fn error_code_label(error: &agent_client_protocol::Error) -> &'static str {
    match error.code {
        ErrorCode::InternalError => "internal_error",
        // ... see 06-type-reference.md for the full table
        _ => "unknown",
    }
}
// Safe: tracing::warn!(code = error_code_label(&error), "prompt failed")
// Risky: tracing::warn!(%error, "prompt failed")  // may log agent text
```

For local diagnostics (not persisted), `%error` is fine. For anything that hits
the event store or syncs to clients, use the code label.

## 14. `session/list` before `session/load` — but tolerate failures

When resuming a persisted session, call `session/list` first (if supported) to
confirm the session still exists. But `session/list` can be flaky on some
agents — **fall through to `session/load` on list failure** rather than giving
up. Only treat "id not in list" as "create new"; treat "list errored" as "try
load anyway."

## 15. MCP is additive — never block session creation

MCP server config load/parse errors must yield an **empty** server list, never
fail `session/new`. MCP is a capability add-on; a broken `mcp.json` shouldn't
prevent the user from chatting.

```rust
let servers = match load_mcp_config(path) {
    Ok(s) => s,
    Err(e) => { tracing::warn!(%e, "mcp load failed; continuing without mcp"); Vec::new() }
};
```

## 16. Terminal env filtering — strip secrets and hijack vars

When creating a terminal, the agent supplies env vars and you inherit the
daemon's env. **Filter both:**

- **Daemon env:** strip secrets (API keys, `*_TOKEN`, your app's internal vars)
  so they don't leak to agent-spawned commands.
- **Agent env:** strip hijack vars (`LD_PRELOAD`, `DYLD_INSERT_LIBRARIES`,
  `LD_LIBRARY_PATH`, etc.) that could redirect execution.

Then merge the filtered daemon env (safe allowlist) with the filtered agent env.

## 17. Terminal output bounding — don't OOM

Terminals can produce gigabytes of output. Cap retained output per terminal
(discard oldest complete UTF-8 prefix when over the limit) and cap the number
of terminals per session (e.g. 16). Use `String::ceil_char_boundary` when
draining so you don't split a multi-byte UTF-8 character.

## 18. The `on_receive_request!()` / `on_receive_notification!()` macros are mandatory

These are **required workaround arguments** for the lack of return-type notation
in Rust (rust-lang/rust#109417). You must pass them as the second argument to
every `.on_receive_request(...)` and `.on_receive_notification(...)` call.
Omitting them is a compile error. They expand to a `Box::pin` wrapper.

## 19. `SessionId` is cheap to clone

`SessionId` wraps `Arc<str>`. Cloning is a refcount bump, not a string copy.
Pass it by value (`.clone()`) into requests rather than threading `&SessionId`
everywhere — it's idiomatic and cheap.

## 20. Advertise only the capabilities you implement

If you set `client_capabilities.fs.read_text_file(true)` in `initialize`, the
agent **will** send you `fs/read_text_file` requests. If you didn't register a
handler, the agent hangs or errors. Match caps to handlers exactly. Same for
`write_text_file` and `terminal`.

## 21. `ToolCall` vs `ToolCallUpdate` — different access paths & optionality

ACP has two tool-call shapes that share field names but differ in structure:

- **`ToolCall`** (in `SessionUpdate::ToolCall`) has **direct, mostly non-optional
  fields**: `tc.title: String`, `tc.kind: ToolKind`, `tc.status: ToolCallStatus`,
  `tc.locations: Vec<ToolCallLocation>`. Access directly: `tc.title`.
- **`ToolCallUpdate`** (in `SessionUpdate::ToolCallUpdate` **and**
  `RequestPermissionRequest.tool_call`) has a **flattened `fields:
  ToolCallUpdateFields`** where **every field is `Option<...>`**: access via
  `upd.fields.title: Option<String>`, `upd.fields.kind: Option<ToolKind>`, etc.

The permission request reuses the **update** shape (all optional) because the
agent may omit fields irrelevant to the permission decision. If you try
`request.tool_call.title` (without `.fields`) it won't compile; if you assume
`kind` is non-optional you'll get a type error. See
[06-type-reference.md](06-type-reference.md) §"`ToolCall` vs `ToolCallUpdate`"
for the full field-by-field table.

## 22. `raw_input` / `raw_output` are `serde_json::Value`, not strings

`ToolCall.raw_input` and `ToolCallUpdateFields.raw_input` (and `raw_output`)
are `Option<serde_json::Value>`, **not** `Option<String>`. To get a display
string, use `as_ref()` + `to_string()`:

```rust
let command: String = tool_call.raw_input.as_ref()
    .map_or(String::new, ToString::to_string);
```

`as_deref()` won't work — `serde_json::Value` doesn't implement `Deref<Target=str>`.
