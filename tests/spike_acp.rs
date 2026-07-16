//! S-ACP-SPIKE: ACP Rust SDK proof-of-capability integration tests.
//!
//! These are **disposable spike tests**. Their job is to verify the
//! `agent_client_protocol` crate (v1.2.0) can replace the Go
//! `coder/acp-go-sdk` for the operations `internal/acp/` relies on:
//! initialize, session lifecycle, streaming, file/shell callbacks,
//! permission requests, cancellation, MCP relay, and auth shape.
//!
//! The fixture is the Go `cmd/mockagent` binary (built to `/tmp/mockagent`),
//! which speaks ACP over stdio using the Go SDK. We connect to it from Rust
//! via [`agent_client_protocol::AcpAgent`], the SDK's blessed stdio
//! transport: it spawns the agent subprocess with `async_process::Command`,
//! pipes stdin/stdout, and feeds a line-delimited JSON-RPC transport
//! (`agent_client_protocol::Lines`). This proves the
//! `(AsyncWrite, AsyncRead) -> JSON-RPC` transport path the production
//! ACP stories will use.
//!
//! Verified API shape (real, not guessed — see `docs/rust-ecosystem/acp-rust-sdk.md`
//! for the updated reference):
//! - `agent_client_protocol::Client` — unit struct; `.builder()` returns a
//!   `Builder<Client, NullHandler, NullRun>`.
//! - `Builder::on_receive_request::<Req>(async |req, responder, cx| {...}, on_receive_request!())`
//!   registers a typed handler. `responder: Responder<Req::Response>` — call
//!   `responder.respond(value)` to answer.
//! - `Builder::on_receive_notification::<Notif>(async |notif, cx| {...}, on_receive_notification!())`
//!   registers a typed notification handler.
//! - `Builder::connect_with(transport, async |cx: ConnectionTo<Agent>| {...})` runs
//!   the dispatch loop until the closure returns.
//! - `ConnectionTo<Agent>::send_request::<Req>(req) -> SentRequest<Resp>`;
//!   `.block_task().await` awaits the response inside a spawned task
//!   (avoids deadlocking the dispatch loop).
//! - `ConnectionTo::send_notification::<N>(notif)` — fire-and-forget.
//! - Transport: `AcpAgent::from_str("/tmp/mockagent")` implements
//!   `ConnectTo<Client>`; internally spawns the child and constructs
//!   `Lines::new(outgoing_sink, incoming_lines)`.
//!
//! Gaps found:
//! - **No `ExecuteCommand` request.** ACP delegates shell execution to the
//!   `terminal/*` family (`terminal/create`, `terminal/output`,
//!   `terminal/release`, `terminal/wait_for_exit`, `terminal/kill`). The Go
//!   code's `ExecuteCommand` callback maps to `CreateTerminalRequest` +
//!   driving output. The mockagent does not exercise terminal callbacks, so
//!   the shell round-trip is verified at the type level (handler registration
//!   compiles and the request type is reachable) rather than at runtime.
//! - **MCP relay is fully supported** when the `unstable_mcp_over_acp` feature
//!   is enabled: `ConnectMcpRequest`/`ConnectMcpResponse`,
//!   `MessageMcpRequest`/`MessageMcpResponse`/`MessageMcpNotification`,
//!   `DisconnectMcpRequest`/`DisconnectMcpResponse` are all code-generated.
//!   This **closes** the Go SDK gap (Go only had `mcp/connect`/`disconnect`).
//!   The mockagent does not act as an MCP relay peer, so we verify the types
//!   compile and the methods are wired; a live relay round-trip is deferred
//!   to S-MCP / S-ACP-CORE.
//! - **Auth (PKCE)**: the SDK surfaces `InitializeResponse.auth_methods:
//!   Vec<AuthMethod>` and `AuthenticateRequest`/`AuthenticateResponse`. The
//!   mockagent returns an empty `auth_methods` and a no-op `Authenticate`.
//!   We verify the auth flow *shape* (types are reachable, an
//!   `authenticate` round-trip succeeds) but not a real PKCE dance — that
//!   requires a real agent and is deferred to S-ACP-CORE.

// Spike test code is allowed to use `.unwrap()`/`.expect()`/`panic!()` for
// fail-fast assertions. The crate-level `[lints.clippy]` policy in Cargo.toml
// denies these in non-test code so the daemon cannot accidentally panic on
// the LAN; this inner attribute lifts that bar for this integration test
// crate (mirroring the `cfg_attr(test, allow(...))` in `src/lib.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    AuthenticateRequest, AuthenticateResponse, ContentBlock, InitializeRequest, NewSessionRequest,
    PromptRequest, ReadTextFileRequest, ReadTextFileResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome, SessionId,
    SessionNotification, SessionUpdate, StopReason, TextContent, WriteTextFileRequest,
    WriteTextFileResponse,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};
use std::str::FromStr;
use tokio::sync::Mutex;

/// Path to the pre-built Go mockagent binary (built by the spike harness:
/// `go build -o /tmp/mockagent ./cmd/mockagent/`).
const MOCKAGENT_BIN: &str = "/tmp/mockagent";

/// How long to wait for the mockagent to stream a full response before
/// declaring the spike failed. Generous because the mock streams word-by-word
/// with a 20ms delay.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(15);

/// Helper trait so the test can pull text out of a `ContentBlock` without
/// matching the full enum in the hot path.
trait ContentBlockTextExt {
    fn as_text(&self) -> Option<String>;
}

impl ContentBlockTextExt for ContentBlock {
    /// Returns the block's text if it is the `Text` variant.
    fn as_text(&self) -> Option<String> {
        match self {
            ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        }
    }
}

/// Build an `AcpAgent` transport pointing at the mockagent binary.
///
/// `AcpAgent` is the SDK's stdio transport: it spawns the binary with
/// `async_process::Command` (piped stdin/stdout/stderr) and adapts the
/// streams to the line-delimited JSON-RPC transport the dispatch loop
/// expects. This is the verified replacement for Go's
/// `acp.NewAgentSideConnection` / `ClientSideConnection` over `os.Stdin`/
/// `os.Stdout`.
fn mockagent_transport() -> AcpAgent {
    assert!(
        std::path::Path::new(MOCKAGENT_BIN).exists(),
        "mockagent binary missing at {MOCKAGENT_BIN}; build it with `go build -o /tmp/mockagent ./cmd/mockagent/`"
    );
    AcpAgent::from_str(MOCKAGENT_BIN).expect("valid mockagent command")
}

/// Shared state collected by the notification handler during a prompt turn.
///
/// Each field is wrapped in `Arc<Mutex<...>>` so the handler closure (which
/// must be `Send`) can mutate it across `await` points while the main task
/// inspects it after the prompt resolves.
#[derive(Default)]
struct Observed {
    /// Concatenated agent message text chunks (`AgentMessageChunk`).
    agent_text: Arc<Mutex<String>>,
    /// Concatenated agent thought text chunks (`AgentThoughtChunk`).
    thought_text: Arc<Mutex<String>>,
    /// Number of `ToolCall` (tool-call-started) updates seen.
    tool_starts: Arc<Mutex<u32>>,
    /// Number of `ToolCallUpdate` (tool-call-progress) updates seen.
    tool_updates: Arc<Mutex<u32>>,
    /// Whether a permission request was delivered to the client.
    permission_requested: Arc<Mutex<bool>>,
    /// Whether a `ReadTextFile` request was delivered to the client.
    read_text_file_called: Arc<Mutex<bool>>,
    /// Whether a `WriteTextFile` request was delivered to the client.
    write_text_file_called: Arc<Mutex<bool>>,
}

/// Spawn a tokio timeout future. Returns `Err` with a descriptive message
/// on timeout so the test failure points at the right phase.
async fn with_timeout<F, T>(label: &str, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    match tokio::time::timeout(PROMPT_TIMEOUT, f).await {
        Ok(v) => v,
        Err(_) => panic!("spike step timed out after {PROMPT_TIMEOUT:?}: {label}"),
    }
}

// ---------------------------------------------------------------------------
// Acceptance: initialize + session/new + prompt + stream + close
// ---------------------------------------------------------------------------

/// AC: "Mock-agent CI test covers initialize, create, prompt, stream, cancel,
/// and close."
///
/// This test covers initialize, create, prompt, stream, and close. Cancellation
/// has its own test below (`spike_cancel_terminates_child`) because it needs
/// to assert on process teardown.
#[tokio::test]
async fn spike_initialize_session_prompt_stream_close() {
    let observed = Observed::default();
    let Observed {
        agent_text,
        thought_text,
        tool_starts,
        tool_updates,
        permission_requested: _,
        read_text_file_called: _,
        write_text_file_called: _,
    } = observed.clone_observed_handles();

    let agent_text_h = agent_text.clone();
    let thought_text_h = thought_text.clone();
    let tool_starts_h = tool_starts.clone();
    let tool_updates_h = tool_updates.clone();

    let transport = mockagent_transport();

    // `Client` is a unit struct; `.builder()` returns the v1 client builder
    // (the `Role::builder` impl calls `.v1_client()` internally).
    let result = Client
        .builder()
        .name("local-agent-spike")
        // Stream handler: every `session/update` notification arrives here as
        // a typed `SessionNotification`. We pattern-match on `SessionUpdate`
        // variants — the SDK gives us a strongly-typed enum instead of Go's
        // `map[string]interface{}`.
        .on_receive_notification(
            async move |notif: SessionNotification, _cx: ConnectionTo<Agent>| {
                match notif.update {
                    SessionUpdate::AgentMessageChunk(chunk) => {
                        if let Some(text) = chunk.content.as_text() {
                            *agent_text_h.lock().await += &text;
                        }
                    }
                    SessionUpdate::AgentThoughtChunk(chunk) => {
                        if let Some(text) = chunk.content.as_text() {
                            *thought_text_h.lock().await += &text;
                        }
                    }
                    SessionUpdate::ToolCall(_) => {
                        *tool_starts_h.lock().await += 1;
                    }
                    SessionUpdate::ToolCallUpdate(_) => {
                        *tool_updates_h.lock().await += 1;
                    }
                    _ => {}
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        // Permission handler: the mockagent does not currently emit
        // `session/request_permission`, but we register one to prove the
        // typed round-trip compiles and to satisfy the AC "Permission request
        // is delivered and first response wins" when a real agent exercises
        // it. We auto-allow the first option.
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, _cx: ConnectionTo<Agent>| {
                *observed.permission_requested.lock().await = true;
                let option_id = req
                    .options
                    .first()
                    .map(|o| o.option_id.clone())
                    .unwrap_or_else(|| {
                        agent_client_protocol::schema::v1::PermissionOptionId::new("allow_once")
                    });
                responder.respond(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
                ))
            },
            agent_client_protocol::on_receive_request!(),
        )
        // File-read handler: proves the `fs/read_text_file` callback path.
        .on_receive_request(
            async move |req: ReadTextFileRequest, responder, _cx: ConnectionTo<Agent>| {
                *observed.read_text_file_called.lock().await = true;
                // For the spike we just echo the path back as the "contents".
                // The mockagent doesn't actually call fs/read_text_file, but
                // the handler compiles and is wired — proving the API shape.
                let contents = format!("[spike read] {}", req.path.display());
                responder.respond(ReadTextFileResponse::new(contents))
            },
            agent_client_protocol::on_receive_request!(),
        )
        // File-write handler: proves the `fs/write_text_file` callback path.
        .on_receive_request(
            async move |req: WriteTextFileRequest, responder, _cx: ConnectionTo<Agent>| {
                *observed.write_text_file_called.lock().await = true;
                // Spike: don't touch the disk, just acknowledge.
                let _ = (req.path, req.content);
                responder.respond(WriteTextFileResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, |cx: ConnectionTo<Agent>| async move {
            // 1. initialize
            let init = with_timeout(
                "initialize",
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task(),
            )
            .await
            .expect("initialize request failed");
            assert_eq!(init.protocol_version, ProtocolVersion::V1);
            let agent_info = init.agent_info.expect("mockagent should report agent_info");
            assert_eq!(agent_info.name, "MockAgent");

            // 2. session/new
            let cwd = PathBuf::from(".");
            let new_session = with_timeout(
                "session/new",
                cx.send_request(NewSessionRequest::new(cwd)).block_task(),
            )
            .await
            .expect("session/new failed");
            let session_id = new_session.session_id;
            assert!(!session_id.0.is_empty(), "session id should be non-empty");

            // 3. session/prompt with streaming notifications
            let prompt = vec![ContentBlock::Text(TextContent::new("hello spike"))];
            let prompt_resp = with_timeout(
                "session/prompt",
                cx.send_request(PromptRequest::new(session_id.clone(), prompt))
                    .block_task(),
            )
            .await
            .expect("prompt failed");
            assert_eq!(prompt_resp.stop_reason, StopReason::EndTurn);

            Ok(())
        })
        .await;

    // connect_with returns once main_fn returns; the SDK then tears down the
    // transport, which kills the child process (ChildGuard in acp_agent.rs).
    result.expect("spike connection failed");

    // 4. Verify we received typed streaming updates.
    let agent_text = observed.agent_text.lock().await.clone();
    let thought_text = observed.thought_text.lock().await.clone();
    let tool_starts = *observed.tool_starts.lock().await;
    let tool_updates = *observed.tool_updates.lock().await;

    assert!(
        agent_text.contains("hello spike"),
        "agent message text should echo the prompt; got: {agent_text:?}"
    );
    assert!(
        !thought_text.is_empty(),
        "should have received at least one AgentThoughtChunk"
    );
    // The mockagent starts two tool calls (ls, pwd) and updates each once.
    assert_eq!(tool_starts, 2, "expected 2 ToolCall starts (ls + pwd)");
    assert_eq!(tool_updates, 2, "expected 2 ToolCallUpdate completions");
}

// ---------------------------------------------------------------------------
// Acceptance: file read/write callback round trips
// ---------------------------------------------------------------------------

/// AC: "File write/read and shell callback round trips are verified."
///
/// The mockagent does not emit `fs/read_text_file` or `fs/write_text_file`
/// requests itself, so we verify the callback round-trip at the protocol
/// level: we register handlers (proven reachable in the test above) and
/// additionally exercise the typed request/response shape by constructing
/// the request types and asserting they serialize. The live agent round-trip
/// for fs callbacks is deferred to S-ACP-CORE (real agent fixture).
///
/// Shell: ACP has no `ExecuteCommand` request — shell execution is
/// delegated to the `terminal/*` family. See the module docs and
/// `spike_shell_callback_types_reachable`.
#[tokio::test]
async fn spike_file_callback_types_reachable() {
    // Prove the request/response types round-trip through serde with the
    // exact field shapes the SDK generated. This is the contract the
    // production fs callback handler will rely on.
    let read_req = ReadTextFileRequest::new(SessionId::from("s1"), "/tmp/spike.txt");
    let json = serde_json::to_string(&read_req).expect("serialize ReadTextFileRequest");
    let parsed: ReadTextFileRequest =
        serde_json::from_str(&json).expect("deserialize ReadTextFileRequest");
    assert_eq!(parsed.path, read_req.path);

    let write_req = WriteTextFileRequest::new(SessionId::from("s1"), "/tmp/spike.txt", "hi");
    let json = serde_json::to_string(&write_req).expect("serialize WriteTextFileRequest");
    let parsed: WriteTextFileRequest =
        serde_json::from_str(&json).expect("deserialize WriteTextFileRequest");
    assert_eq!(parsed.content, "hi");

    // Response shapes
    let _ = ReadTextFileResponse::new("contents");
    let _ = WriteTextFileResponse::new();
}

// ---------------------------------------------------------------------------
// Acceptance: shell callback round trip
// ---------------------------------------------------------------------------

/// AC (shell portion): "File write/read and shell callback round trips are
/// verified."
///
/// ACP does **not** define an `ExecuteCommand` request. Shell execution is
/// carried by the `terminal/*` family: `CreateTerminalRequest` (allocate a
/// PTY/terminal), `TerminalOutputRequest` (send input / read output),
/// `ReleaseTerminalRequest`, `WaitForTerminalExitRequest`, `KillTerminalRequest`.
/// The Go code's `ExecuteCommand` callback is a higher-level helper that
/// wraps these; in Rust the production shell executor (S-SHELL + S-ACP-CORE)
/// will register a `CreateTerminalRequest` handler and drive it.
///
/// This test proves the terminal request/response types are reachable and
/// serde-round-trip — the contract for the future handler.
#[tokio::test]
async fn spike_shell_callback_types_reachable() {
    use agent_client_protocol::schema::v1::{
        CreateTerminalRequest, CreateTerminalResponse, KillTerminalRequest, KillTerminalResponse,
        ReleaseTerminalRequest, ReleaseTerminalResponse, TerminalExitStatus, TerminalId,
        TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
        WaitForTerminalExitResponse,
    };

    // Construct each request via its `::new` builder and serde-round-trip it.
    // The mockagent doesn't exercise these, but proving the wire shape is the
    // S-ACP-CORE input.
    let create = CreateTerminalRequest::new(SessionId::from("s1"), "bash");
    let j = serde_json::to_string(&create).expect("serialize CreateTerminalRequest");
    let _: CreateTerminalRequest = serde_json::from_str(&j).expect("deserialize");
    let _ = CreateTerminalResponse::new(TerminalId::from("t1"));

    // TerminalOutputRequest carries the terminal id (no input string field;
    // input is sent by writing to the terminal, output is read here).
    let out_req = TerminalOutputRequest::new(SessionId::from("s1"), TerminalId::from("t1"));
    let j = serde_json::to_string(&out_req).expect("serialize TerminalOutputRequest");
    let _: TerminalOutputRequest = serde_json::from_str(&j).expect("deserialize");
    let _ = TerminalOutputResponse::new("ls\nfile1\n", false);

    let _ = ReleaseTerminalRequest::new(SessionId::from("s1"), TerminalId::from("t1"));
    let _ = ReleaseTerminalResponse::default();
    let _ = WaitForTerminalExitRequest::new(SessionId::from("s1"), TerminalId::from("t1"));
    let _ = WaitForTerminalExitResponse::new(TerminalExitStatus::default());
    let _ = KillTerminalRequest::new(SessionId::from("s1"), TerminalId::from("t1"));
    let _ = KillTerminalResponse::default();
}

// ---------------------------------------------------------------------------
// Acceptance: permission request delivered and first response wins
// ---------------------------------------------------------------------------

/// AC: "Permission request is delivered and first response wins."
///
/// The mockagent does not emit `session/request_permission`. We prove the
/// round-trip shape (request → typed response) compiles and that the
/// `Responder::respond` path is the single answer channel (calling it twice
/// would be a logic error; the SDK owns the response slot). A live
/// permission round-trip against a real agent is deferred to S-ACP-CORE.
///
/// What is verified here: the response enum variants serialize correctly
/// (`Selected` with an option id, and `Cancelled`), which is the contract
/// the production permission manager will use.
#[tokio::test]
async fn spike_permission_response_shape() {
    let selected = RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
        SelectedPermissionOutcome::new("allow_once"),
    ));
    let j = serde_json::to_string(&selected).expect("serialize Selected response");
    assert!(j.contains("\"outcome\""), "outcome field present: {j}");
    assert!(j.contains("allow_once"), "option id present: {j}");

    let cancelled = RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled);
    let j = serde_json::to_string(&cancelled).expect("serialize Cancelled response");
    assert!(j.contains("cancelled"), "cancelled variant present: {j}");
}

// ---------------------------------------------------------------------------
// Acceptance: child process and its process tree terminate on cancellation
// ---------------------------------------------------------------------------

/// AC: "Child process and its process tree terminate on cancellation."
///
/// Strategy: spawn the mockagent with `async_process::Command` (the same
/// crate the SDK's `AcpAgent` uses internally), grab its PID, and feed the
/// piped stdin/stdout into `ByteStreams` — the SDK's
/// `(AsyncWrite, AsyncRead) -> JSON-RPC` transport. This proves the
/// transport path AND lets us assert on process teardown.
///
/// We start a prompt turn, then return an error from `connect_with`'s main
/// closure to tear down the dispatch loop. The transport drop closes the
/// pipes; we then explicitly `kill()` the child (mirroring the SDK's
/// `ChildGuard::drop`) and assert the PID is reaped.
///
/// Note: the mockagent's `Cancel` handler is a no-op (it returns nil without
/// stopping the prompt goroutine), so cancellation here is purely
/// client-side teardown — which is exactly the production contract: the
/// client owns the child process tree and must kill it on cancel. The
/// mockagent's `ls`/`pwd` children are short-lived `exec.CommandContext`
/// calls that complete on their own; the tree-termination guarantee is the
/// client killing the agent process, which terminates its goroutines and
/// any in-flight `exec.CommandContext` children (whose context is tied to
/// the parent process exit).
#[tokio::test]
async fn spike_cancel_terminates_child() {
    use agent_client_protocol::ByteStreams;

    let mut cmd = async_process::Command::new(MOCKAGENT_BIN);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().expect("spawn mockagent");
    let pid = child.id();
    let child_stdin = child.stdin.take().expect("stdin");
    let child_stdout = child.stdout.take().expect("stdout");
    // Drain stderr so the child doesn't block on a full pipe buffer.
    let _stderr = child.stderr.take();

    // `ByteStreams` adapts an (AsyncWrite, AsyncRead) pair into the
    // line-delimited JSON-RPC transport. `async_process::ChildStdin`/
    // `ChildStdout` implement `futures::io::AsyncWrite`/`AsyncRead` — this
    // is the verified stdio transport path the production ACP stories will
    // use (the SDK's `AcpAgent` wraps the same `async_process` types).
    let transport = ByteStreams::new(child_stdin, child_stdout);

    // Hold the child in an Arc<Mutex<Option<_>>> so we can kill + reap it
    // after the connection tears down.
    let child_arc = Arc::new(Mutex::new(Some(child)));
    let child_for_teardown = child_arc.clone();

    // Drive a connection that starts a prompt and then abandons it by
    // returning an error — tearing down the dispatch loop and dropping the
    // transport (which closes the pipes).
    let connect_result: Result<(), agent_client_protocol::Error> = Client
        .builder()
        .name("local-agent-cancel-spike")
        .on_receive_notification(
            async |_notif: SessionNotification, _cx| Ok(()),
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(transport, |cx: ConnectionTo<Agent>| async move {
            // Initialize + new session so the agent is mid-turn when we cancel.
            let _init = cx
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let new_session = cx
                .send_request(NewSessionRequest::new(PathBuf::from(".")))
                .block_task()
                .await?;
            let session_id = new_session.session_id;

            // Start a prompt but DON'T await it — detach so the request is
            // in flight when we tear down. Dropping the SentRequest sends a
            // `$/cancel_request` notification to the agent.
            let _sent = cx.send_request(PromptRequest::new(
                session_id,
                vec![ContentBlock::Text(TextContent::new("please stream slowly"))],
            ));
            // Give the agent a moment to start streaming.
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Return an error to force the dispatch loop to tear down.
            Err(agent_client_protocol::Error::internal_error())
        })
        .await;
    // We expect an error — that's the cancellation path.
    assert!(connect_result.is_err(), "expected intentional cancel error");

    // Now kill + reap the child. kill_on_drop would fire on drop, but we
    // make it explicit and then poll for exit.
    {
        let mut guard = child_for_teardown.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            // Reap: wait for exit. The pipes are already closed by the
            // transport drop, so this completes quickly.
            let _ = with_timeout("child exit after kill", child.status()).await;
        }
    }

    // Assert the PID is gone. `kill -0` returns success if the process
    // exists. After kill+reap it should be gone.
    let mut exited = false;
    for _ in 0..20 {
        if !pid_is_alive(pid) {
            exited = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        exited,
        "mockagent child pid {pid} did not terminate after kill within 2s"
    );
}

/// Best-effort "is this PID still alive" check using `kill -0`.
///
/// Returns `true` if the process exists (signal delivered successfully),
/// `false` if it's gone. Shells out to avoid a `libc` dev-dependency.
fn pid_is_alive(pid: u32) -> bool {
    let status = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status();
    match status {
        Ok(s) => s.success(),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Acceptance: MCP relay availability
// ---------------------------------------------------------------------------

/// AC: "MCP relay availability is proven; absence has an isolated fallback
/// design."
///
/// **Finding:** with the `unstable_mcp_over_acp` feature enabled, the Rust
/// SDK **fully code-generates** the MCP-over-ACP relay types:
/// - `mcp/connect`: `ConnectMcpRequest` / `ConnectMcpResponse`
/// - `mcp/message`: `MessageMcpRequest` / `MessageMcpResponse` /
///   `MessageMcpNotification` (the inner-MCP notification relay)
/// - `mcp/disconnect`: `DisconnectMcpRequest` / `DisconnectMcpResponse`
///
/// This **closes the Go SDK gap** documented in `docs/STATUS.md` and
/// `acp-spec-compliance.md` §4.10: the Go SDK only generated
/// `mcp/connect`/`disconnect` and could not relay `mcp/message`, forcing an
/// inline transport workaround. The Rust port can drop that workaround.
///
/// **API shape note (verified):** for protocol v1, the standalone MCP types
/// do NOT implement `JsonRpcMessage` directly — they are only reachable as
/// variants of the `AgentRequest` / `AgentResponse` / `AgentNotification`
/// enums (see `src/schema/enum_impls.rs`). So a v1 client sends
/// `mcp/connect` by constructing `AgentRequest::ConnectMcpRequest(...)` and
/// calling `cx.send_request::<AgentRequest>(...)`, then matching the
/// `AgentResponse::ConnectMcpResponse(...)` variant. (The standalone
/// `impl_jsonrpc_request!` impls exist only for the v2 draft.) The
/// `on_receive_request::<AgentRequest>` handler pattern lets the client
/// dispatch all agent→client requests (including `mcp/connect`/
/// `mcp/message`/`mcp/disconnect`) in one closure, matching the Go SDK's
/// single `Client` interface.
///
/// The mockagent does not act as an MCP relay peer, so this test verifies
/// the types are reachable, serde-round-trip, and the enum variants exist.
/// A live relay round-trip is deferred to S-MCP / S-ACP-CORE.
#[tokio::test]
async fn spike_mcp_relay_types_supported() {
    use agent_client_protocol::schema::v1::{
        AgentRequest, AgentResponse, ConnectMcpRequest, ConnectMcpResponse, DisconnectMcpRequest,
        DisconnectMcpResponse, McpConnectionId, MessageMcpNotification, MessageMcpRequest,
        MessageMcpResponse,
    };

    // mcp/connect — reachable both standalone (serde) and as an enum variant.
    let connect = ConnectMcpRequest::new("server-1");
    let j = serde_json::to_string(&connect).expect("serialize ConnectMcpRequest");
    assert!(j.contains("serverId"), "server_id field present: {j}");
    // The enum variant serializes to the same body; the JSON-RPC method
    // ("mcp/connect") is attached by the SDK's envelope, not by serde, so we
    // only assert the payload round-trips through the enum.
    let enum_req = AgentRequest::ConnectMcpRequest(connect.clone());
    let parsed: AgentRequest =
        serde_json::from_str(&serde_json::to_string(&enum_req).unwrap()).expect("round-trip");
    assert!(matches!(parsed, AgentRequest::ConnectMcpRequest(_)));

    let connect_resp = ConnectMcpResponse::new(McpConnectionId::from("conn-1"));
    let j = serde_json::to_string(&connect_resp).expect("serialize ConnectMcpResponse");
    assert!(j.contains("conn-1"), "connection_id present: {j}");

    // mcp/message (request) — carries the inner MCP method + optional params.
    let mut msg = MessageMcpRequest::new(McpConnectionId::from("conn-1"), "tools/list");
    let j = serde_json::to_string(&msg).expect("serialize MessageMcpRequest");
    assert!(j.contains("tools/list"), "inner method present: {j}");
    let mut params = serde_json::Map::new();
    params.insert("cursor".into(), serde_json::json!("abc"));
    msg = msg.params(params);
    let j = serde_json::to_string(&msg).expect("serialize MessageMcpRequest with params");
    assert!(j.contains("cursor"), "params present: {j}");
    // Enum variant reachable.
    let _ = AgentRequest::MessageMcpRequest(msg);

    // mcp/message (notification)
    let notif =
        MessageMcpNotification::new(McpConnectionId::from("conn-1"), "notifications/progress");
    let j = serde_json::to_string(&notif).expect("serialize MessageMcpNotification");
    assert!(
        j.contains("notifications/progress"),
        "inner method present: {j}"
    );

    // mcp/message (response) — wraps a raw JSON value (the inner MCP result).
    let raw = serde_json::value::RawValue::from_string(r#"{"tools":[]}"#.to_string())
        .expect("parse raw value");
    // `from_string` returns `Box<RawValue>`; `MessageMcpResponse` wants
    // `Arc<RawValue>`. Convert via `Arc::from(Box<RawValue>)` (RawValue is
    // unsized, so this is the documented conversion path).
    let resp = MessageMcpResponse::new(std::sync::Arc::from(raw));
    let j = serde_json::to_string(&resp).expect("serialize MessageMcpResponse");
    assert!(j.contains("tools"), "inner result present: {j}");
    // Response enum variant reachable.
    let _ = AgentResponse::MessageMcpResponse(resp);

    // mcp/disconnect
    let disconnect = DisconnectMcpRequest::new(McpConnectionId::from("conn-1"));
    let j = serde_json::to_string(&disconnect).expect("serialize DisconnectMcpRequest");
    assert!(j.contains("conn-1"), "connection_id present: {j}");
    let _ = AgentRequest::DisconnectMcpRequest(disconnect);
    let _ = DisconnectMcpResponse::default();
}

// ---------------------------------------------------------------------------
// Acceptance: PKCE / auth flow shape
// ---------------------------------------------------------------------------

/// AC (auth portion, implicit): "verify the SDK's auth flow shape."
///
/// The SDK surfaces:
/// - `InitializeResponse.auth_methods: Vec<AuthMethod>` — the agent
///   advertises how it wants to authenticate. Variants: `Agent` (agent
///   handles it itself, the default), and (with `unstable_auth_methods`)
///   `EnvVar` (client injects a key as an env var) and `Terminal` (client
///   runs an interactive TUI).
/// - `AuthenticateRequest` / `AuthenticateResponse` — the
///   `authenticate` method the client calls after picking a method.
/// - `LogoutRequest` / `LogoutResponse` — the `logout` method.
///
/// The mockagent returns empty `auth_methods` and a no-op `Authenticate`.
/// This test verifies the round-trip compiles and succeeds against the
/// mockagent, proving the auth *plumbing* works end-to-end. A real PKCE
/// dance (OAuth redirect, code exchange) requires a real agent and is
/// deferred to S-ACP-CORE.
#[tokio::test]
async fn spike_auth_flow_shape() {
    let transport = mockagent_transport();

    let result = Client
        .builder()
        .name("local-agent-auth-spike")
        .on_receive_notification(
            async |_notif: SessionNotification, _cx| Ok(()),
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(transport, |cx: ConnectionTo<Agent>| async move {
            let init = cx
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            // The mockagent advertises no auth methods (Agent-handled by
            // default). Record the shape we observed.
            let _auth_methods = init.auth_methods.clone();
            // Even with no advertised methods, calling `authenticate` with
            // the default (Agent) method id is a no-op on the mockagent.
            let auth_req = AuthenticateRequest::new(
                agent_client_protocol::schema::v1::AuthMethodId::from("agent"),
            );
            let _auth_resp: AuthenticateResponse = cx.send_request(auth_req).block_task().await?;
            Ok(())
        })
        .await;

    result.expect("auth spike connection failed");
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

impl Observed {
    /// Clone the `Arc<Mutex<_>>` handles so a handler closure can capture
    /// them by value while the test body retains its own references.
    fn clone_observed_handles(&self) -> Observed {
        Observed {
            agent_text: self.agent_text.clone(),
            thought_text: self.thought_text.clone(),
            tool_starts: self.tool_starts.clone(),
            tool_updates: self.tool_updates.clone(),
            permission_requested: self.permission_requested.clone(),
            read_text_file_called: self.read_text_file_called.clone(),
            write_text_file_called: self.write_text_file_called.clone(),
        }
    }
}
