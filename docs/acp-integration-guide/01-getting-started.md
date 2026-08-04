# 01 — Getting Started: Transport, Connection & Handler Model

This document covers the foundational mental model: how to spawn an agent, build
the SDK connection, register handlers, and run the dispatch loop. Every later
document assumes you understand this.

## The mental model

The Rust SDK uses a **builder + handler + dispatch loop** pattern:

1. You build a `Client` via `Client.builder()`.
2. You register **typed handlers** for each request/notification the agent will
   send you (file reads, permission prompts, terminal ops, session updates).
3. You call `.connect_with(transport, main_fn)`, which:
   - starts the JSON-RPC dispatch loop (reads lines from the transport, decodes,
     routes to your handlers), and
   - runs your `main_fn` closure, which receives a `ConnectionTo<Agent>` handle
     and drives the session lifecycle (initialize, prompt, cancel, etc.).
4. `connect_with` returns when `main_fn` returns OR the transport closes.

**Critical invariant:** the dispatch loop runs on the task that awaits
`connect_with`. Your handlers and your `main_fn` all run in that task's context.
**Never block the dispatch loop** — see [Non-blocking callbacks](#non-blocking-callbacks)
below.

## Roles & entry point

```rust
use agent_client_protocol::{Client, Agent, ConnectionTo};

// `Client` and `Agent` are unit structs (zero-sized markers).
// `Client::builder()` returns `Builder<Client, NullHandler, NullRun>`,
// pre-configured for protocol v1.
Client.builder()
    .name("my-app")           // clientInfo.name (MUST be non-empty — see Gotchas)
    // .on_receive_request(...) — register handlers (see below)
    // .on_receive_notification(...)
    .connect_with(transport, async |cx: ConnectionTo<Agent>| {
        // main_fn: drive the session lifecycle here.
        // `cx` is your handle for sending requests/notifications to the agent.
        // Returning from here ends the dispatch loop and tears down the connection.
        Ok(()) // Result<R, agent_client_protocol::Error>
    })
    .await?; // -> Result<R, Error>
```

`Client` = the client role. `Agent` = the agent role. `ConnectionTo<Agent>` =
"my connection **to** the agent" (I'm the client, sending requests to the agent).
If you were implementing the agent side, you'd use `ConnectionTo<Client>`.

## Transport: spawning the agent subprocess

The SDK uses **`futures::io::{AsyncRead, AsyncWrite}`** (the `futures-io` crate
traits), **NOT** tokio's `AsyncRead`/`AsyncWrite`. This matters for which
`Command`/`Child` types you use. There are two verified transport paths:

### Path A: `AcpAgent` (blessed, simplest)

`AcpAgent` parses a command string, spawns the child with
`async_process::Command`, pipes stdin/stdout, and adapts to the line-delimited
JSON-RPC transport. It wraps the child in a `ChildGuard` that **kills the
process on drop**.

```rust
use std::str::FromStr;
use agent_client_protocol::AcpAgent;

// Bare command string (Unix). `from_str` uses `shell_words::split`.
let agent = AcpAgent::from_str("/usr/local/bin/claude")?;
// With args:
let agent = AcpAgent::from_str("/usr/local/bin/claude --acp-protocol-version 1")?;

Client.builder()
    .name("my-app")
    .connect_with(agent, |cx| async { /* main_fn */ })
    .await?;
```

**Windows caveat:** `AcpAgent::from_str` parses with `shell_words::split`, which
treats backslashes as escapes and mangles Windows paths (`C:\tmp\agent.exe`).
On Windows, pass the command as JSON to bypass the shell-words parser:

```rust
#[cfg(windows)]
{
    let json = format!(
        r#"{{"type":"stdio","name":"agent","command":{},"args":[],"env":[]}}"#,
        serde_json::to_string(&bin_path)?
    );
    AcpAgent::from_str(&json)?
}
```

### Path B: `ByteStreams` (manual, for PID tracking / custom spawn)

Use this when you need control over the child process (process-group isolation,
stderr capture, cwd, env filtering, explicit kill). You spawn the child
yourself with `async_process::Command` (NOT `tokio::process::Command` — the
`async_process::ChildStdin`/`ChildStdout` streams implement `futures::io`).

```rust
use async_process::{Command, Stdio};
use agent_client_protocol::ByteStreams;

let mut command = Command::new("/usr/local/bin/claude");
command
    .args(["--acp-protocol-version", "1"])
    .current_dir(&workspace_path)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())   // capture for diagnostics
    .kill_on_drop(true);      // safety net, but see process-group note below
let mut child = command.spawn()?;

let stdin = child.stdin.take().expect("stdin pipe");
let stdout = child.stdout.take().expect("stdout pipe");
// Drain stderr on a separate task so it can't block the transport:
if let Some(stderr) = child.stderr.take() {
    tokio::spawn(async move {
        use futures_util::AsyncReadExt;
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        // ... retain a tail for diagnostics ...
    });
}

let transport = ByteStreams::new(stdin, stdout);

Client.builder()
    .name("my-app")
    // ... handlers ...
    .connect_with(transport, |cx| async { /* main_fn */ })
    .await?;
```

**Process-group isolation (Unix):** `kill_on_drop` only terminates the direct
child. Agents often spawn descendant processes (language servers, build tools).
To reap the whole tree on shutdown, put the child in its own process group
before exec, then kill the group on teardown:

```rust
// Build with std::process::Command first to set the process group,
// then convert to async_process::Command.
let mut std_cmd = std::process::Command::new(&agent_binary);
std_cmd.args(&agent_args).current_dir(&workspace_path);
configure_process_group(&mut std_cmd); // sets setpgid(0,0) via pre_exec
let mut command = Command::from(std_cmd);
// ... pipes, spawn ...

// On teardown (after connect_with returns):
#[cfg(unix)]
if let Ok(pid) = i32::try_from(child.id()) {
    kill_process_group(pid); // kill(-pgid, SIGKILL)
}
let _ = child.kill();
let _ = child.status().await; // reap
```

See [07-gotchas.md](07-gotchas.md) §"Process tree teardown" for the full
shutdown sequence.

## Handler registration

The agent sends you **requests** (expect a response) and **notifications**
(fire-and-forget). You register one handler per typed request/notification.

### Request handler

```rust
use agent_client_protocol::{Agent, ConnectionTo, Responder};
use agent_client_protocol::schema::v1::{ReadTextFileRequest, ReadTextFileResponse};

Client.builder()
    .name("my-app")
    .on_receive_request(
        async |request: ReadTextFileRequest, responder: Responder<ReadTextFileResponse>,
                _cx: ConnectionTo<Agent>| {
            // `responder.respond(value)` is the SINGLE answer channel.
            // First response wins; subsequent calls are ignored.
            responder.respond(ReadTextFileResponse::new(file_contents));
            Ok(()) // returning Err stops the dispatch loop
        },
        agent_client_protocol::on_receive_request!(), // required macro arg
    )
```

### Notification handler

```rust
use agent_client_protocol::schema::v1::SessionNotification;

Client.builder()
    .name("my-app")
    .on_receive_notification(
        async |notification: SessionNotification, _cx: ConnectionTo<Agent>| {
            // No response. Returning Err stops the dispatch loop.
            handle_session_update(notification).await;
            Ok(())
        },
        agent_client_protocol::on_receive_notification!(), // required macro arg
    )
```

### The `on_receive_request!()` / `on_receive_notification!()` macros

These are **required workaround arguments**. The SDK's builder uses return-type
notation to type-match handlers, which Rust doesn't yet support
(rust-lang/rust#109417). The macros expand to a `Box::pin` wrapper that bridges
the gap. **You must pass them as the second argument to every
`on_receive_request` / `on_receive_notification` call.** Omitting them is a
compile error.

### Capturing state in handlers

Handlers are `FnMut` closures. To share state across invocations, **clone an
`Arc` into each handler** before registering it:

```rust
let deps = HandlerDeps { /* Arc<dyn WorkspaceManager>, etc. */ };

Client.builder()
    .name("my-app")
    .on_receive_request(
        {
            let deps = deps.clone(); // clone for this handler's captured env
            async move |request: ReadTextFileRequest, responder, _cx| {
                let deps = deps.clone(); // re-clone per invocation (FnMut)
                // ... use deps, call responder.respond(...) ...
                Ok(())
            }
        },
        agent_client_protocol::on_receive_request!(),
    )
    .on_receive_request(
        {
            let deps = deps.clone(); // separate clone for this handler
            async move |request: WriteTextFileRequest, responder, _cx| {
                let deps = deps.clone();
                // ...
                Ok(())
            }
        },
        agent_client_protocol::on_receive_request!(),
    )
```

The double-clone (`{ let deps = deps.clone(); async move |...| { let deps = deps.clone(); ... } }`)
is the idiomatic pattern: the outer clone moves one `Arc` into the closure's
captured environment; the inner clone creates a fresh `Arc` for each invocation
so the closure remains `FnMut` (not `FnOnce`).

## Non-blocking callbacks (CRITICAL)

**The dispatch loop runs on the `connect_with` task.** If a handler `await`s a
long-running operation (a user permission prompt that waits for a device, a
terminal command that runs for minutes), it **blocks dispatch of all other
requests and notifications** — including the streaming updates your UI needs.

**Rule: spawn long-running handler work onto a separate task, return `Ok(())`
from the handler immediately.** Use a bounded `Semaphore` to cap concurrent
callback tasks and a `CancellationToken` to abort them when the session closes.

```rust
use agent_client_protocol::{JsonRpcResponse, Responder};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

struct HandlerDeps {
    cancellation: CancellationToken,
    callback_slots: Arc<Semaphore>, // e.g. Semaphore::new(16)
    // ... workspace, permissions, etc. ...
}

/// Reserve one callback slot without blocking dispatch.
fn callback_permit(deps: &HandlerDeps) -> Option<OwnedSemaphorePermit> {
    deps.callback_slots.clone().try_acquire_owned().ok()
}

/// Run a callback until it completes or the session closes.
fn spawn_callback<F>(cancellation: CancellationToken, permit: OwnedSemaphorePermit, future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let _permit = permit; // held for the callback's lifetime
        tokio::select! {
            () = cancellation.cancelled() => {}
            () = future => {}
        }
    });
}

/// Handler helper: spawn a Result-returning callback that maps errors to
/// JSON-RPC internal-error responses.
fn spawn_result_callback<T, F, Fut>(deps: HandlerDeps, responder: Responder<T>, work: F)
where
    T: JsonRpcResponse,
    F: FnOnce(HandlerDeps) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, MyError>> + Send + 'static,
{
    let Some(permit) = callback_permit(&deps) else {
        // At capacity: respond with an internal error so the agent isn't hung.
        let _ = responder.respond_with_internal_error("callback capacity exceeded");
        return;
    };
    spawn_callback(deps.cancellation.clone(), permit, async move {
        match work(deps).await {
            Ok(response) => { let _ = responder.respond(response); }
            Err(error) => { let _ = responder.respond_with_internal_error(error); }
        }
    });
}
```

Then every handler becomes a one-liner that spawns and returns immediately:

```rust
.on_receive_request(
    {
        let deps = deps.clone();
        async move |request: ReadTextFileRequest, responder, _cx| {
            spawn_result_callback(deps.clone(), responder, move |deps| async move {
                read_text_file(deps, request).await
            });
            Ok(()) // dispatch loop continues immediately
        }
    },
    agent_client_protocol::on_receive_request!(),
)
```

**Why `try_acquire_owned` (not `acquire_owned`)?** If the semaphore is full,
`acquire_owned` would `await` — blocking dispatch. `try_acquire_owned` returns
immediately; at capacity you respond with an internal error so the agent
receives a failure instead of hanging.

**`respond_with_internal_error`:** `Responder<T>` has this helper when
`T: JsonRpcResponse`. It sends a JSON-RPC error response with code -32603.
Your error type needs an `Into<agent_client_protocol::Error>` or you pass a
string. Check the SDK's `Error::internal_error()` constructor.

## Sending requests & notifications to the agent

From `main_fn` (or any code holding a `ConnectionTo<Agent>`):

```rust
// Send a request and await the response.
// `.block_task()` spawns a task to await WITHOUT deadlocking the dispatch loop.
let init: InitializeResponse = cx
    .send_request(InitializeRequest::new(ProtocolVersion::V1))
    .block_task()
    .await?;

// Fire-and-forget notification.
cx.send_notification(CancelNotification::new(session_id.clone()))?;
```

### `SentRequest<T>` — the handle returned by `send_request`

`cx.send_request(req)` returns a `SentRequest<T>` (not the response directly).
You must consume it via one of:

| Method | Behavior |
|--------|----------|
| `.block_task().await` | Spawns a task that awaits the response; returns `Result<T, Error>`. **Preferred** — safe inside `main_fn` because it doesn't block the dispatch loop. |
| `.detach()` | Keep the request running on the peer; ignore the response. |
| `.cancel()` | Send `$/cancel_request` to the peer. |
| `Drop` (let it go out of scope) | **Auto-sends `$/cancel_request`** and discards the response. |

**To select over a pending request** (e.g., race prompt completion against a
cancel command), pin it and poll inside `tokio::select!`:

```rust
let prompt = cx.send_request(PromptRequest::new(session_id, blocks)).block_task();
tokio::pin!(prompt);
loop {
    tokio::select! {
        reply = &mut prompt => { /* PromptResponse */ }
        cmd = command_rx.recv() => { /* handle cancel/close */ }
    }
}
```

See [03-prompts-streaming-cancellation.md](03-prompts-streaming-cancellation.md)
for the full prompt-turn pattern.

## Putting it together: minimal connection skeleton

```rust
use std::sync::Arc;
use agent_client_protocol::{Client, Agent, ConnectionTo};
use agent_client_protocol::schema::v1::SessionNotification;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub async fn run_agent_session(agent_binary: &str, workspace: &str) -> Result<(), Box<dyn std::error::Error>> {
    let transport = build_transport(agent_binary, workspace)?; // AcpAgent or ByteStreams
    let deps = HandlerDeps {
        cancellation: CancellationToken::new(),
        callback_slots: Arc::new(Semaphore::new(16)),
        // ... your workspace/permission managers ...
    };

    let result = Client.builder()
        .name("my-app")
        .on_receive_notification(
            {
                let deps = deps.clone();
                async move |notif: SessionNotification, _cx: ConnectionTo<Agent>| {
                    let deps = deps.clone();
                    handle_session_update(&deps, notif).await;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        // ... register request handlers (see 04-client-handlers.md) ...
        .connect_with(transport, move |cx: ConnectionTo<Agent>| async move {
            // main_fn: initialize, create session, drive prompt loop, teardown.
            // See 02-session-lifecycle.md and 03-prompts-streaming-cancellation.md.
            Ok(())
        })
        .await;

    deps.cancellation.cancel(); // abort any in-flight callback tasks
    result.map_err(|e| format!("ACP connection: {e}").into())
}
```

## Next

- [02-session-lifecycle.md](02-session-lifecycle.md) — what goes inside `main_fn`:
  initialize, session new/load, capability caching.
- [04-client-handlers.md](04-client-handlers.md) — the request handlers you
  register before `connect_with`.
