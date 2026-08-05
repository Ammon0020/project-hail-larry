//! Mock ACP agent for testing (Rust port of `cmd/mockagent/main.go`).
//!
//! Speaks the ACP protocol over stdio using the `agent_client_protocol` SDK.
//! Streams canned text responses word-by-word, runs real shell commands
//! (`ls`/`pwd`), and reports tool call results — simulating a real agent
//! without needing an API key.
//!
//! Usage: `mockagent`
//! (reads ACP from stdin, writes ACP to stdout, logs to stderr)
//!
//! This is a test-fixture binary, not the production daemon, so the
//! crate-level panic/unwrap clippy denies are lifted here (mirroring the
//! `cfg_attr(test, allow(...))` in `src/lib.rs` and the integration-test
//! crates).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, AuthenticateRequest, AuthenticateResponse, CancelNotification,
    CloseSessionRequest, CloseSessionResponse, ContentBlock, ContentChunk, Implementation,
    InitializeRequest, InitializeResponse, ListSessionsRequest, ListSessionsResponse,
    LogoutRequest, LogoutResponse, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, ResumeSessionRequest, ResumeSessionResponse, SessionConfigId,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigOptionValue,
    SessionConfigSelectOption, SessionConfigSelectOptions, SessionConfigValueId, SessionId,
    SessionNotification, SessionUpdate, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse, StopReason,
    TextContent, ToolCall, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{
    on_receive_notification, on_receive_request, Agent, Client, ConnectionTo, Error, Responder,
    Stdio,
};
use tokio::sync::Mutex;

/// `MOCKAGENT_NO_MODE_CAP`, when set to a non-empty value, makes the mock agent
/// NOT advertise the `mode`-category `profile` config option. Contract tests
/// use this to exercise the client's prompt-injection fallback branch (where
/// the client skips `session/set_config_option` and injects profile
/// instructions into the prompt instead).
const ENV_NO_MODE_CAP: &str = "MOCKAGENT_NO_MODE_CAP";

/// `MOCKAGENT_EXIT_AFTER_INIT`, when set to a non-empty value, makes the mock
/// agent exit with a non-zero code immediately after the first `session/new`
/// completes. Rust tests use this to exercise the terminal-outcome watcher
/// path for unexpected post-startup actor exits.
const ENV_EXIT_AFTER_INIT: &str = "MOCKAGENT_EXIT_AFTER_INIT";

/// The `SessionConfigOption` id the Rust client sends via
/// `session/set_config_option` to switch the active profile (S-PROF-ACP).
const PROFILE_CONFIG_ID: &str = "profile";

/// Prefixes the mock's first streamed reply chunk with the active profile so
/// contract tests can assert the client sent
/// `set_config_option { configId: profile, value: X }` by observing the mock's
/// output. Picked over a stderr log line because the existing ACP test harness
/// drains stderr without inspecting it, while streamed agent message text is
/// surfaced through the conversation pipeline.
///
/// Format: `[profile: <value>] ` (with trailing space).
const PROFILE_MARKER_PREFIX: &str = "[profile: ";

/// Per-word streaming delay, matching the Go mock's 20ms cadence.
const STREAM_DELAY: Duration = Duration::from_millis(20);

/// Shared state across handlers: session id -> last received profile value.
type Profiles = Arc<Mutex<HashMap<String, String>>>;

/// Build the `mode`-category `profile` select option the client uses to gate
/// the ACP send path. A single placeholder value is advertised for UX
/// completeness; `SetSessionConfigOption` accepts arbitrary profile ids
/// without validating against this list.
fn profile_config_option(current_value: &str) -> SessionConfigOption {
    SessionConfigOption::select(
        SessionConfigId::from(PROFILE_CONFIG_ID),
        "Profile",
        SessionConfigValueId::new(current_value),
        SessionConfigSelectOptions::Ungrouped(vec![SessionConfigSelectOption::new(
            SessionConfigValueId::new("default"),
            "Default",
        )]),
    )
    .category(SessionConfigOptionCategory::Mode)
}

/// Generate a random session id: `"mock-" + 16 hex chars` (matching the Go
/// mock's `crypto/rand` 8-byte hex encoding).
fn random_session_id() -> String {
    // `rand::random::<u64>()` → 16 zero-padded hex chars (8 bytes).
    format!("mock-{:016x}", rand::random::<u64>())
}

/// Run a shell command and return its stdout output. On Windows uses `cmd /c`;
/// on Unix uses `sh -c`. The command is a hardcoded internal mock command
/// (`ls`/`dir`/`pwd`/`cd`), not user input.
fn run_shell_command(cmd: &str) -> String {
    let output = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/c", cmd]).output()
    } else {
        Command::new("sh").args(["-c", cmd]).output()
    };
    match output {
        Ok(out) => {
            // `ls`/`pwd` write to stdout; surface stderr too on failure.
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            if out.status.success() {
                stdout
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                format!("error: exit {:?} stderr: {stderr}", out.status.code())
            }
        }
        Err(err) => format!("error: {err}"),
    }
}

/// Stream text word-by-word with a delay between chunks, simulating a real
/// agent's streaming output. Each word is sent as a separate
/// `AgentMessageChunk` notification; a trailing space is added after every
/// word except the last (matching the Go `streamText` behavior).
async fn stream_text(cx: &ConnectionTo<Client>, sid: &SessionId, text: &str, delay: Duration) {
    let words: Vec<&str> = text.split_whitespace().collect();
    let last = words.len().saturating_sub(1);
    for (i, word) in words.iter().enumerate() {
        let chunk = if i < last {
            format!("{word} ")
        } else {
            (*word).to_string()
        };
        let update = SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new(chunk),
        )));
        let _ = cx.send_notification(SessionNotification::new(sid.clone(), update));
        tokio::time::sleep(delay).await;
    }
}

/// Send a `SessionUpdate` notification for the given session, ignoring errors
/// (the Go mock discards them with `_ = a.conn.SessionUpdate(...)`).
fn send_update(cx: &ConnectionTo<Client>, sid: &SessionId, update: SessionUpdate) {
    let _ = cx.send_notification(SessionNotification::new(sid.clone(), update));
}

// Single linear mock-agent sequence — splitting would obscure the flow.
#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() {
    // Suppress SDK diagnostic logging — stderr is for our own logs. With no
    // `tracing` subscriber installed, `tracing` macros are no-ops, matching
    // the Go mock's `slog.NewTextHandler(io.Discard, nil)`.
    // (No subscriber is registered here.)

    // Default to advertising the mode/profile capability; suppress only when
    // the no-cap env var is explicitly set, so contract tests can exercise the
    // client's prompt-injection fallback branch.
    let mode_cap = std::env::var(ENV_NO_MODE_CAP).map_or(true, |v| v.is_empty());
    let profiles: Profiles = Arc::new(Mutex::new(HashMap::new()));

    let result = Agent
        .builder()
        .name("mockagent")
        // initialize
        .on_receive_request(
            async move |_req: InitializeRequest,
                        responder: Responder<InitializeResponse>,
                        _cx: ConnectionTo<Client>| {
                responder.respond(
                    InitializeResponse::new(ProtocolVersion::V1)
                        .agent_info(Implementation::new("MockAgent", "1.0.0"))
                        .agent_capabilities(AgentCapabilities::new().load_session(false)),
                )
            },
            on_receive_request!(),
        )
        // session/new
        .on_receive_request(
            async move |_req: NewSessionRequest,
                        responder: Responder<NewSessionResponse>,
                        _cx: ConnectionTo<Client>| {
                    let id = random_session_id();
                    let mut resp = NewSessionResponse::new(SessionId::from(id));
                    // Advertise the mode/profile config option so the Rust
                    // client's capability gate takes the
                    // `session/set_config_option` branch. Suppressed when
                    // MOCKAGENT_NO_MODE_CAP is set so the prompt-injection
                    // fallback is testable.
                    if mode_cap {
                        resp = resp.config_options(vec![profile_config_option("")]);
                    }
                    // Simulate an unexpected crash after a successful
                    // initialize + session/new. The Rust actor has already
                    // sent readiness; the terminal watcher must transition
                    // the session to Failed and append an AgentExited event.
                    // Exit BEFORE responding, matching the Go mock's
                    // `os.Exit(1)` at main.go:142-144.
                    if std::env::var(ENV_EXIT_AFTER_INIT).is_ok_and(|v| !v.is_empty())
                    {
                        std::process::exit(1);
                    }
                    responder.respond(resp)
            },
            on_receive_request!(),
        )
        // session/prompt
        .on_receive_request(
            {
                let profiles = Arc::clone(&profiles);
                async move |req: PromptRequest,
                            responder: Responder<PromptResponse>,
                            cx: ConnectionTo<Client>| {
                    let sid = req.session_id.clone();

                    // Extract the user's text from the prompt content blocks.
                    let mut user_text = String::new();
                    for block in &req.prompt {
                        if let ContentBlock::Text(t) = block {
                            user_text.push_str(&t.text);
                        }
                    }

                    // 1. Emit a thought.
                    send_update(
                        &cx,
                        &sid,
                        SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new("Analyzing the request..."),
                        ))),
                    );

                    // 2. Start a tool call — list directory contents.
                    let list_cmd = if cfg!(target_os = "windows") { "dir" } else { "ls" };
                    send_update(
                        &cx,
                        &sid,
                        SessionUpdate::ToolCall(
                            ToolCall::new(ToolCallId::new("tool_ls"), "List directory")
                                .kind(ToolKind::Execute)
                                .status(ToolCallStatus::InProgress)
                                .raw_input(serde_json::json!({ "command": list_cmd })),
                        ),
                    );

                    let ls_output = run_shell_command(list_cmd);
                    send_update(
                        &cx,
                        &sid,
                        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                            ToolCallId::new("tool_ls"),
                            ToolCallUpdateFields::new()
                                .status(ToolCallStatus::Completed)
                                .title("Run ls — completed")
                                .raw_output(serde_json::json!({
                                    "exitCode": 0,
                                    "output": ls_output,
                                })),
                        )),
                    );

                    // 3. Stream the first part of the response. Prefix with
                    //    the active profile marker so contract tests can
                    //    assert the client sent set_config_option.
                    let mut first_chunk = format!(
                        "I received your message: {user_text:?}\n\nHere's what I found in the current directory:\n{ls_output}\n"
                    );
                    {
                        let profiles = profiles.lock().await;
                        if let Some(profile) = profiles.get(sid.0.as_ref()) {
                            first_chunk = format!("{PROFILE_MARKER_PREFIX}{profile}] {first_chunk}");
                        }
                    }
                    stream_text(&cx, &sid, &first_chunk, STREAM_DELAY).await;

                    // 4. Run `pwd` as another tool call.
                    let pwd_cmd = if cfg!(target_os = "windows") { "cd" } else { "pwd" };
                    send_update(
                        &cx,
                        &sid,
                        SessionUpdate::ToolCall(
                            ToolCall::new(ToolCallId::new("tool_pwd"), "Print working directory")
                                .kind(ToolKind::Execute)
                                .status(ToolCallStatus::InProgress)
                                .raw_input(serde_json::json!({ "command": pwd_cmd })),
                        ),
                    );

                    let pwd_output = run_shell_command(pwd_cmd);
                    send_update(
                        &cx,
                        &sid,
                        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                            ToolCallId::new("tool_pwd"),
                            ToolCallUpdateFields::new()
                                .status(ToolCallStatus::Completed)
                                .title("Run pwd — completed")
                                .raw_output(serde_json::json!({
                                    "exitCode": 0,
                                    "output": pwd_output,
                                })),
                        )),
                    );

                    // 5. Stream the final part.
                    let final_chunk = format!(
                        "\nThe current working directory is: {}\n\nAll done!",
                        pwd_output.trim()
                    );
                    stream_text(&cx, &sid, &final_chunk, STREAM_DELAY).await;

                    // 6. Return EndTurn.
                    responder.respond(PromptResponse::new(StopReason::EndTurn))
                }
            },
            on_receive_request!(),
        )
        // session/set_config_option — records the profile value per session.
        .on_receive_request(
            {
                let profiles = Arc::clone(&profiles);
                async move |req: SetSessionConfigOptionRequest,
                            responder: Responder<SetSessionConfigOptionResponse>,
                            _cx: ConnectionTo<Client>| {
                    let session_id = req.session_id.0.to_string();
                    let config_id = req.config_id.0.to_string();
                    let value: String = match &req.value {
                        SessionConfigOptionValue::ValueId { value } => value.0.to_string(),
                        SessionConfigOptionValue::Boolean { value } => value.to_string(),
                        // non_exhaustive: unknown value shapes default to empty
                        // (matching the Go mock's no-op when neither branch matches).
                        _ => String::new(),
                    };

                    // Only the `profile` (mode-category) option is meaningful
                    // for the profile-over-ACP contract tests; other options
                    // are accepted without error. The handler succeeds even
                    // when the capability was not advertised
                    // (MOCKAGENT_NO_MODE_CAP), so a misbehaving client still
                    // gets a clean response — the capability gate is the
                    // client's responsibility.
                    if config_id == PROFILE_CONFIG_ID {
                        let mut profiles = profiles.lock().await;
                        profiles.insert(session_id.clone(), value.clone());
                        // Echo the recorded value to stderr as a secondary,
                        // greppable signal for harnesses that capture child
                        // stderr. (tracing is a no-op without a subscriber;
                        // the primary signal is the `[profile: X]` marker.)
                        tracing::info!(
                            session_id = %session_id,
                            profile = %value,
                            "set_config_option profile recorded"
                        );
                    }

                    // Echo back the updated profileConfigOption(value).
                    responder.respond(SetSessionConfigOptionResponse::new(vec![
                        profile_config_option(&value),
                    ]))
                }
            },
            on_receive_request!(),
        )
        // session/close — delete the profile record.
        .on_receive_request(
            {
                let profiles = Arc::clone(&profiles);
                async move |req: CloseSessionRequest,
                            responder: Responder<CloseSessionResponse>,
                            _cx: ConnectionTo<Client>| {
                    let mut profiles = profiles.lock().await;
                    profiles.remove(req.session_id.0.as_ref());
                    drop(profiles);
                    responder.respond(CloseSessionResponse::default())
                }
            },
            on_receive_request!(),
        )
        // session/cancel — no-op accept (notification, no response).
        .on_receive_notification(
            async |_notif: CancelNotification, _cx: ConnectionTo<Client>| { Ok(()) },
            on_receive_notification!(),
        )
        // session/list — method-not-found (LoadSession: false).
        .on_receive_request(
            async move |_req: ListSessionsRequest,
                        responder: Responder<ListSessionsResponse>,
                        _cx: ConnectionTo<Client>| {
                responder.respond_with_error(Error::method_not_found())
            },
            on_receive_request!(),
        )
        // session/resume — method-not-found (LoadSession: false).
        .on_receive_request(
            async move |_req: ResumeSessionRequest,
                        responder: Responder<ResumeSessionResponse>,
                        _cx: ConnectionTo<Client>| {
                responder.respond_with_error(Error::method_not_found())
            },
            on_receive_request!(),
        )
        // session/set_mode — no-op accept.
        .on_receive_request(
            async move |_req: SetSessionModeRequest,
                        responder: Responder<SetSessionModeResponse>,
                        _cx: ConnectionTo<Client>| { responder.respond(SetSessionModeResponse::default()) },
            on_receive_request!(),
        )
        // authenticate — no-op.
        .on_receive_request(
            async move |_req: AuthenticateRequest,
                        responder: Responder<AuthenticateResponse>,
                        _cx: ConnectionTo<Client>| { responder.respond(AuthenticateResponse::default()) },
            on_receive_request!(),
        )
        // logout — no-op.
        .on_receive_request(
            async move |_req: LogoutRequest,
                        responder: Responder<LogoutResponse>,
                        _cx: ConnectionTo<Client>| { responder.respond(LogoutResponse::default()) },
            on_receive_request!(),
        )
        // Block until the incoming transport closes (stdin EOF), matching the
        // Go mock's `<-conn.Done()`. Returning early would tear down the
        // connection before requests are handled.
        .connect_with(Stdio::new(), |cx: ConnectionTo<Client>| async move {
            cx.incoming_closed().await;
            Ok(())
        })
        .await;

    if let Err(err) = result {
        // The connection ended with an error; exit non-zero so test harnesses
        // can distinguish a clean EOF from a transport failure.
        tracing::error!(error = %err, "mockagent connection ended with error");
        std::process::exit(1);
    }
}
