//! ACP agent process startup, SDK connection construction, and session
//! new/load resolution.
//!
//! The actor task is the sole owner of the `ConnectionTo<Agent>`; it never
//! receives the live/dormant session maps and reports failure through a
//! terminal-outcome channel consumed by lifecycle orchestration.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    ClientCapabilities, FileSystemCapabilities, Implementation, InitializeRequest,
    InitializeResponse, ListSessionsRequest, LoadSessionRequest, McpServer, NewSessionRequest,
    SessionConfigOption, SessionId, SessionNotification,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Agent, ByteStreams, Client as SdkClient, ConnectionTo};
use agent_client_protocol_conductor::{ConductorImpl, ProxiesAndAgent};
use agent_client_protocol_polyfill::mcp_over_acp::McpOverAcpPolyfill;
use async_process::Command;
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio_util::sync::CancellationToken;

mod setup;
mod turn;

use super::super::providers::{find_model_config_id, find_profile_config_id, SessionCaps};
use super::diagnostics::{spawn_stderr_drain, StderrTail};
use super::events::handle_session_notification;
use super::handlers::{
    cancel_terminals, create_terminal, kill_terminal, read_text_file, release_terminal,
    request_permission, spawn_respond_callback, spawn_result_callback, terminal_output,
    wait_for_terminal_exit, write_text_file, HandlerDeps,
};
use super::mcp::load_session_mcp_servers;
use crate::config::AgentInfo;
use crate::events::SharedEventBus;
use crate::interfaces::{AppError, PermissionManager, WorkspaceManager};
use crate::procutil::{configure_process_group, ProcessGroupCleanup};

/// Bounded command channel capacity for one actor.
pub(super) const ACTOR_COMMAND_CAPACITY: usize = 32;
/// Maximum callback requests an agent can make concurrently per session.
const MAX_CALLBACK_TASKS: usize = 16;

/// ACP `clientInfo.name` — must be non-empty; agents (e.g. Mistral Vibe) forward
/// it into provider metadata that rejects blank values. Matches Go transport.
pub(super) const ACP_CLIENT_NAME: &str = "LocalAgentInterface";
/// ACP `clientInfo.version` — same Go parity constraint as [`ACP_CLIENT_NAME`].
pub(super) const ACP_CLIENT_VERSION: &str = "1.0";

/// Monotonic actor identity for staleness checks in terminal watchers.
static ACTOR_ID: AtomicU64 = AtomicU64::new(0);

pub(super) use turn::{ActorCommand, ActorExit};

/// Startup handshake result returned before the session is published.
pub(super) struct ActorStartup {
    pub(super) caps: SessionCaps,
    pub(super) model_config_id: Option<String>,
    /// Mode/profile config option id (`None` when the agent lacks the
    /// capability — prompt-injection fallback applies in `context.rs`).
    pub(super) profile_config_id: Option<String>,
    /// Resolved agent-side ACP session id (from load or new).
    pub(super) acp_session_id: String,
}

/// Constructor-only dependencies for spawning an actor. The actor never
/// receives the session registry; failure is reported via [`TerminalOutcome`].
pub(super) struct Config {
    pub(super) local_session_id: String,
    pub(super) agent: AgentInfo,
    pub(super) workspace_id: String,
    pub(super) workspace_path: PathBuf,
    pub(super) permissions: Arc<dyn PermissionManager>,
    pub(super) workspaces: Arc<dyn WorkspaceManager>,
    pub(super) event_bus: SharedEventBus,
    pub(super) stderr_tail: Arc<Mutex<StderrTail>>,
    pub(super) prompt_cancel: Arc<std::sync::atomic::AtomicBool>,
    /// Optional `mcp.json` path passed through to session/new and session/load.
    pub(super) mcp_config_path: Option<PathBuf>,
    /// Profile middleware used to resolve the MCP server policy at session setup.
    pub(super) profiles: Arc<super::super::profile::ProfileMiddleware>,
    /// Durable agent ACP session id to attempt `session/load` with (empty =
    /// always `session/new`). Cleared on rebind.
    pub(super) persisted_acp_session_id: String,
    /// The model id selected by the user at session creation or rebind. Sent to
    /// the agent via `session/set_config_option` (model category) after session
    /// setup so the agent uses the requested model instead of its default.
    pub(super) model_id: String,
}

/// Opaque, cloneable handle to a running actor. Carries the command sender
/// and a monotonic identity so terminal watchers can skip stale exits after
/// a rebind replaces the actor.
#[derive(Clone)]
pub(super) struct Handle {
    commands: mpsc::Sender<ActorCommand>,
    id: u64,
}

impl Handle {
    /// Return a clone of the command sender for enqueuing actor commands.
    pub(super) fn commands(&self) -> mpsc::Sender<ActorCommand> {
        self.commands.clone()
    }

    /// Monotonic identity for staleness checks.
    pub(super) fn id(&self) -> u64 {
        self.id
    }

    /// Test-only constructor for a handle whose command channel is already
    /// closed, so callers can prove lifecycle surfaces a send error.
    #[cfg(test)]
    pub(super) fn dead() -> Self {
        let (commands, _) = mpsc::channel(1);
        Self {
            commands,
            id: ACTOR_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}

/// Terminal outcome reported by the actor task to lifecycle orchestration.
pub(super) enum TerminalOutcome {
    /// Actor exited after acknowledging a Close command (normal teardown).
    Closed,
    /// Actor exited unexpectedly (startup failure or post-startup crash).
    /// Lifecycle owns registry mutation and event publication for this case.
    Failed(AppError),
}

/// All channels returned by [`spawn`] for the readiness/registration handshake
/// and terminal-outcome watching.
pub(super) struct Spawned {
    pub(super) ready: oneshot::Receiver<Result<ActorStartup, AppError>>,
    pub(super) registered: oneshot::Sender<()>,
    pub(super) terminal: oneshot::Receiver<TerminalOutcome>,
    pub(super) handle: Handle,
}

/// Spawn an actor task and return channels for the readiness/registration
/// handshake plus a terminal-outcome watcher.
pub(super) fn spawn(config: Config, capacity: usize) -> Spawned {
    let (commands, receiver) = mpsc::channel(capacity);
    let (ready_tx, ready_rx) = oneshot::channel();
    let (registered_tx, registered_rx) = oneshot::channel();
    let (terminal_tx, terminal_rx) = oneshot::channel();
    let id = ACTOR_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let handle = Handle { commands, id };
    tokio::spawn(run_actor(
        config,
        receiver,
        ready_tx,
        registered_rx,
        terminal_tx,
    ));
    Spawned {
        ready: ready_rx,
        registered: registered_tx,
        terminal: terminal_rx,
        handle,
    }
}

async fn run_actor(
    config: Config,
    mut commands: mpsc::Receiver<ActorCommand>,
    ready: oneshot::Sender<Result<ActorStartup, AppError>>,
    registered: oneshot::Receiver<()>,
    terminal: oneshot::Sender<TerminalOutcome>,
) {
    let mut ready = Some(ready);
    let mut registered = Some(registered);
    let result = run_actor_inner(&config, &mut commands, &mut ready, &mut registered).await;
    match result {
        Ok(ActorExit::Closed(close_result)) => {
            let _ = close_result.send(());
            let _ = terminal.send(TerminalOutcome::Closed);
        }
        Err(error) => {
            if let Some(ready) = ready.take() {
                // Startup failure: lifecycle sees the error via `ready` and
                // handles registry/event bookkeeping. The terminal channel
                // still closes so a watcher cannot hang.
                let _ = ready.send(Err(startup_error(&error, &config.stderr_tail)));
            } else {
                // Post-startup exit: lifecycle's terminal watcher owns
                // registry mutation and AgentExited publication.
                tracing::warn!(error = %error, "ACP session actor ended");
            }
            let _ = terminal.send(TerminalOutcome::Failed(error));
        }
    }
}

/// Add a bounded, line-redacted agent diagnostic to startup failures.
fn startup_error(error: &AppError, stderr_tail: &Arc<Mutex<StderrTail>>) -> AppError {
    let stderr = stderr_tail
        .lock()
        .map_or_else(|_| String::new(), |tail| tail.safe_diagnostic());
    if stderr.is_empty() {
        AppError::internal(error.to_string())
    } else {
        AppError::internal(format!("{error} (agent stderr: {stderr})"))
    }
}

// Actor state machine — splitting would obscure the startup/command transition flow.
#[allow(clippy::too_many_lines)]
async fn run_actor_inner(
    config: &Config,
    commands: &mut mpsc::Receiver<ActorCommand>,
    ready: &mut Option<oneshot::Sender<Result<ActorStartup, AppError>>>,
    registered: &mut Option<oneshot::Receiver<()>>,
) -> Result<ActorExit, AppError> {
    // Build via std::process::Command so we can attach Unix process-group
    // isolation (setpgid) before converting to async-process. kill_on_drop
    // alone only terminates the direct child — descendants of the agent must
    // die with the session on cancel/shutdown.
    let mut std_cmd = std::process::Command::new(&config.agent.command);
    std_cmd
        .args(&config.agent.args)
        .current_dir(&config.workspace_path);
    configure_process_group(&mut std_cmd);
    let mut command = Command::from(std_cmd);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| AppError::internal(format!("spawn ACP agent: {error}")))?;
    // Guard until reaped: dropping the actor (panic / early return) still
    // SIGKILLs the whole Unix process group, not just the agent PID.
    let mut process_group_cleanup = ProcessGroupCleanup::new(Some(child.id()));
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| AppError::internal("ACP agent stdin pipe unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::internal("ACP agent stdout pipe unavailable"))?;
    if let Some(stderr) = child.stderr.take() {
        spawn_stderr_drain(stderr, Arc::clone(&config.stderr_tail));
    }
    let transport = ByteStreams::new(stdin, stdout);
    // Insert a conductor with the MCP-over-ACP polyfill between the client and
    // the agent. For agents that support HTTP MCP but not native ACP, the
    // polyfill advertises mcpCapabilities.acp = true and bridges any
    // McpServer::Acp declarations through a loopback HTTP listener, relaying
    // mcp/connect / mcp/message / mcp/disconnect over ACP. Stdio/Http/Sse
    // servers and native-ACP agents pass through unchanged.
    // TODO: client-side handlers for mcp/connect / message / disconnect are not
    // yet wired, and ServerConfig::to_acp does not yet emit McpServer::Acp.
    // The polyfill is inert until both are addressed (see follow-up story).
    let conductor = ConductorImpl::new_agent(
        "local-agent-conductor",
        ProxiesAndAgent::new(transport).proxy(McpOverAcpPolyfill::http()),
    );
    let terminals = Arc::new(Mutex::new(HashMap::new()));
    let handler_cancel = CancellationToken::new();
    let event_bus = Arc::clone(&config.event_bus);
    let local_session_id = config.local_session_id.clone();
    let prompt_cancel = Arc::clone(&config.prompt_cancel);
    let handler_deps = HandlerDeps {
        local_session_id: config.local_session_id.clone(),
        workspace_id: config.workspace_id.clone(),
        workspace_path: config.workspace_path.clone(),
        workspaces: Arc::clone(&config.workspaces),
        permissions: Arc::clone(&config.permissions),
        event_bus: Arc::clone(&config.event_bus),
        terminals: Arc::clone(&terminals),
        cancellation: handler_cancel.clone(),
        callback_slots: Arc::new(Semaphore::new(MAX_CALLBACK_TASKS)),
    };
    let connected = SdkClient
        .builder()
        .name("local-agent")
        .on_receive_notification(
            {
                let deps = handler_deps.clone();
                async move |notification: SessionNotification, _cx: ConnectionTo<Agent>| {
                    let deps = deps.clone();
                    handle_session_notification(&deps, notification)
                        .await
                        .map_err(|error| {
                            // Returning an SDK error stops dispatch rather than
                            // silently losing a session update after a failed
                            // durable append.
                            tracing::error!(
                                session_id = %deps.local_session_id,
                                error = %error,
                                "failed to persist ACP session update"
                            );
                            agent_client_protocol::Error::internal_error()
                        })
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let deps = handler_deps.clone();
                async move |request: agent_client_protocol::schema::v1::ReadTextFileRequest, responder, _cx: ConnectionTo<Agent>| {
                    // Handlers are FnMut; clone deps for each inbound request.
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP denied file read",
                        move |deps| async move { read_text_file(deps, request).await },
                    );
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let deps = handler_deps.clone();
                async move |request: agent_client_protocol::schema::v1::WriteTextFileRequest, responder, _cx: ConnectionTo<Agent>| {
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP denied file write",
                        move |deps| async move { write_text_file(deps, request).await },
                    );
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let deps = handler_deps.clone();
                // Permission waits for a user device and must not block SDK
                // dispatch. Errors become Cancelled outcomes, not internal errors.
                async move |request: agent_client_protocol::schema::v1::RequestPermissionRequest,
                            responder,
                            _cx: ConnectionTo<Agent>| {
                    spawn_respond_callback(deps.clone(), responder, move |deps| async move {
                        request_permission(deps, request).await
                    });
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let deps = handler_deps.clone();
                async move |request: agent_client_protocol::schema::v1::CreateTerminalRequest, responder, _cx: ConnectionTo<Agent>| {
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP denied terminal create",
                        move |deps| async move { create_terminal(deps, request).await },
                    );
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let deps = handler_deps.clone();
                async move |request: agent_client_protocol::schema::v1::TerminalOutputRequest, responder, _cx: ConnectionTo<Agent>| {
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP terminal output failed",
                        move |deps| async move { terminal_output(&deps, &request) },
                    );
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let deps = handler_deps.clone();
                // Terminal waits can run indefinitely; keep them off the dispatch task.
                async move |request: agent_client_protocol::schema::v1::WaitForTerminalExitRequest,
                            responder,
                            _cx: ConnectionTo<Agent>| {
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP terminal wait failed",
                        move |deps| async move { wait_for_terminal_exit(deps, request).await },
                    );
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let deps = handler_deps.clone();
                async move |request: agent_client_protocol::schema::v1::KillTerminalRequest, responder, _cx: ConnectionTo<Agent>| {
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP terminal kill failed",
                        move |deps| async move { kill_terminal(&deps, &request) },
                    );
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let deps = handler_deps;
                async move |request: agent_client_protocol::schema::v1::ReleaseTerminalRequest, responder, _cx: ConnectionTo<Agent>| {
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP terminal release failed",
                        move |deps| async move { release_terminal(&deps, &request) },
                    );
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(conductor, move |cx: ConnectionTo<Agent>| async move {
            // clientInfo is required by agents that forward name/version into
            // upstream provider metadata (Mistral rejects empty strings).
            let initialize = InitializeRequest::new(ProtocolVersion::V1)
                .client_info(Implementation::new(ACP_CLIENT_NAME, ACP_CLIENT_VERSION))
                .client_capabilities(
                    ClientCapabilities::new()
                        .fs(
                            FileSystemCapabilities::new()
                                .read_text_file(true)
                                .write_text_file(true),
                        )
                        .terminal(true),
                );
            // Keep the InitializeResponse: providers + embeddedContext caps are
            // cached on the session entry so later RPCs can gate without re-probe.
            let init = cx
                .send_request(initialize)
                .block_task()
                .await
                .map_err(|_| agent_client_protocol::Error::internal_error())?;
            let agent_caps = &init.agent_capabilities;
            let session_caps = &agent_caps.session_capabilities;
            let caps = SessionCaps {
                providers_supported: agent_caps.providers.is_some(),
                embedded_context: agent_caps.prompt_capabilities.embedded_context,
                can_list_sessions: session_caps.list.is_some(),
                can_load_session: agent_caps.load_session,
                can_resume_session: session_caps.resume.is_some(),
                can_close_session: session_caps.close.is_some(),
                can_delete_session: session_caps.delete.is_some(),
            };
            tracing::debug!(
                providers_supported = caps.providers_supported,
                embedded_context = caps.embedded_context,
                can_list_sessions = caps.can_list_sessions,
                can_load_session = caps.can_load_session,
                can_resume_session = caps.can_resume_session,
                "ACP initialize capabilities cached"
            );
            // MCP is additive: malformed/missing config must not block session create.
            // Profile MCP-server policy is applied after capability filtering.
            let mcp_servers = load_session_mcp_servers(
                config.mcp_config_path.as_deref(),
                &init.agent_capabilities.mcp_capabilities,
                config.profiles.as_ref(),
                &config.local_session_id,
            )
            .await;
            let (agent_session_id, config_options) = resolve_acp_session(
                &cx,
                &init,
                &config.workspace_path,
                mcp_servers,
                &config.persisted_acp_session_id,
            )
            .await
            .inspect_err(|error| {
                if error.to_string().to_ascii_lowercase().contains("authentication") {
                    tracing::error!(
                        "AGENT AUTHENTICATION REQUIRED: The agent CLI rejected the session request. \
                        Please run `{} login` on the host machine running this daemon \
                        to authenticate your environment.",
                        config.agent.command
                    );
                }
            })?;
            let model_config_id = find_model_config_id(
                config_options.as_deref().unwrap_or(&[]),
                &config.agent.models,
            );
            if model_config_id.is_none() {
                tracing::info!(
                    "agent did not advertise a model config option; switch_model will be unsupported"
                );
            }
            // Profile (mode-category) config option id. `None` means the agent
            // lacks the capability; profile instructions are injected into the
            // prompt context as the fallback (context.rs).
            let profile_config_id =
                find_profile_config_id(config_options.as_deref().unwrap_or(&[]));
            if profile_config_id.is_none() {
                tracing::info!(
                    "agent did not advertise a mode/profile config option; profile will use prompt-injection fallback"
                );
            }
            let acp_session_id = agent_session_id.to_string();
            if let Some(ready) = ready.take() {
                let _ = ready.send(Ok(ActorStartup {
                    caps,
                    model_config_id: model_config_id.clone(),
                    profile_config_id: profile_config_id.clone(),
                    acp_session_id,
                }));
            }
            if let Some(registered) = registered.take() {
                registered
                    .await
                    .map_err(|_| agent_client_protocol::Error::internal_error())?;
            }
            // Send the initial model and profile config options to the agent.
            // Both are best-effort: a failure is logged but does not fail
            // session setup — the agent keeps its defaults if a send fails.
            setup::send_initial_config_options(
                &cx,
                &agent_session_id,
                config,
                model_config_id.as_deref(),
                profile_config_id.as_deref(),
            )
            .await;
            turn::actor_loop(
                cx,
                agent_session_id,
                commands,
                event_bus,
                local_session_id,
                prompt_cancel,
            )
            .await
        })
        .await;
    handler_cancel.cancel();
    cancel_terminals(&terminals);
    // Kill the agent process group (Unix) / child (Windows), then reap.
    // Explicit kill covers the normal close path; ProcessGroupCleanup covers
    // early returns / panics before this point.
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        crate::procutil::kill_process_group(pid);
    }
    let _ = child.kill();
    let _ = child.status().await;
    process_group_cleanup.disarm();
    connected.map_err(|error| AppError::internal(format!("ACP connection: {error}")))
}

/// Reports whether `persisted_id` appears in an agent `session/list` response.
///
/// Pure helper extracted for unit tests (mirrors Go `sessionExists`).
fn session_exists(
    sessions: &[agent_client_protocol::schema::v1::SessionInfo],
    persisted_id: &str,
) -> bool {
    sessions
        .iter()
        .any(|session| session.session_id.to_string() == persisted_id)
}

/// Whether resolve should attempt `session/load` given persisted id + caps.
///
/// Pure gate matching Go `resolveACPSession` before any RPC (list is a
/// separate capability checked by the caller).
fn should_attempt_load(persisted_id: &str, load_session: bool) -> bool {
    !persisted_id.is_empty() && load_session
}

/// Decides load vs new after Initialize, matching Go `resolveACPSession`.
///
/// Flow:
/// 1. If persisted id + load + list: `ListSessions` by cwd; missing → `NewSession`;
///    list error → fall through.
/// 2. If persisted id + load: `LoadSession`; success returns the persisted id.
/// 3. Else / on load failure: `NewSession`.
async fn resolve_acp_session(
    cx: &ConnectionTo<Agent>,
    init: &InitializeResponse,
    workspace_path: &Path,
    mcp_servers: Vec<McpServer>,
    persisted_id: &str,
) -> Result<(SessionId, Option<Vec<SessionConfigOption>>), agent_client_protocol::Error> {
    let can_load = init.agent_capabilities.load_session;
    let can_list = init.agent_capabilities.session_capabilities.list.is_some();

    // When the agent supports session/list, reconcile first: only attempt
    // LoadSession if the agent confirms the session still exists.
    if should_attempt_load(persisted_id, can_load) && can_list {
        match cx
            .send_request(ListSessionsRequest::new().cwd(workspace_path.to_path_buf()))
            .block_task()
            .await
        {
            Ok(listed) => {
                if !session_exists(&listed.sessions, persisted_id) {
                    tracing::info!(
                        local_hint = "acp_session_absent_from_list",
                        "ACP session/list did not include persisted id; creating new session"
                    );
                    return new_acp_session(cx, workspace_path, mcp_servers).await;
                }
                // Session confirmed present — attempt LoadSession below.
            }
            Err(error) => {
                // ListSessions error: fall through to try-load-then-new so we
                // do not regress on agents with flaky list support.
                tracing::info!(
                    error = %error,
                    "ACP session/list failed; falling through to session/load"
                );
            }
        }
    }

    if should_attempt_load(persisted_id, can_load) {
        let load_req = LoadSessionRequest::new(SessionId::new(persisted_id), workspace_path)
            .mcp_servers(mcp_servers.clone());
        match cx.send_request(load_req).block_task().await {
            Ok(loaded) => {
                tracing::info!("ACP session/load succeeded; resuming persisted agent session");
                return Ok((SessionId::new(persisted_id), loaded.config_options));
            }
            Err(error) => {
                tracing::info!(
                    error = %error,
                    "ACP session/load failed; falling back to session/new"
                );
                // Fall through to NewSession on any load error.
            }
        }
    }

    new_acp_session(cx, workspace_path, mcp_servers).await
}

/// Creates a fresh ACP session via `session/new`.
async fn new_acp_session(
    cx: &ConnectionTo<Agent>,
    workspace_path: &Path,
    mcp_servers: Vec<McpServer>,
) -> Result<(SessionId, Option<Vec<SessionConfigOption>>), agent_client_protocol::Error> {
    let session = cx
        .send_request(NewSessionRequest::new(workspace_path).mcp_servers(mcp_servers))
        .block_task()
        .await?;
    tracing::info!("ACP session/new created a new agent session");
    Ok((session.session_id, session.config_options))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(unix)]
    use tempfile::TempDir;

    use super::{session_exists, should_attempt_load, ACP_CLIENT_NAME, ACP_CLIENT_VERSION};

    #[test]
    fn should_attempt_load_requires_persisted_id_and_capability() {
        assert!(should_attempt_load("acp-1", true));
        assert!(!should_attempt_load("", true));
        assert!(!should_attempt_load("acp-1", false));
        assert!(!should_attempt_load("", false));
    }

    #[test]
    fn session_exists_matches_agent_listed_ids() {
        use agent_client_protocol::schema::v1::{SessionId, SessionInfo as AcpListedSession};

        let sessions = vec![
            AcpListedSession::new(SessionId::new("acp-a"), "/ws"),
            AcpListedSession::new(SessionId::new("acp-b"), "/ws"),
        ];
        assert!(session_exists(&sessions, "acp-a"));
        assert!(session_exists(&sessions, "acp-b"));
        assert!(!session_exists(&sessions, "acp-missing"));
        assert!(!session_exists(&[], "acp-a"));
    }

    #[test]
    fn initialize_client_info_is_non_empty() {
        use agent_client_protocol::schema::v1::{Implementation, InitializeRequest};
        use agent_client_protocol::schema::ProtocolVersion;

        let req = InitializeRequest::new(ProtocolVersion::V1)
            .client_info(Implementation::new(ACP_CLIENT_NAME, ACP_CLIENT_VERSION));
        let info = req.client_info.expect("client_info must be set");
        assert!(!info.name.is_empty(), "client name must not be empty");
        assert!(!info.version.is_empty(), "client version must not be empty");
        assert_eq!(info.name, "LocalAgentInterface");
        assert_eq!(info.version, "1.0");
    }

    /// ACP agent spawn uses process-group isolation so descendants die on
    /// shutdown (`kill_on_drop` alone only reaps the direct child).
    #[cfg(unix)]
    #[tokio::test]
    async fn agent_process_group_kill_reaps_descendant() {
        use std::process::Command as StdCommand;

        use crate::procutil::{configure_process_group, ProcessGroupCleanup};

        async fn wait_for_pid_file(path: &Path, timeout: Duration) -> i32 {
            let start = tokio::time::Instant::now();
            loop {
                if let Ok(contents) = std::fs::read_to_string(path) {
                    let trimmed = contents.trim();
                    if !trimmed.is_empty() {
                        return trimmed.parse().expect("numeric descendant PID");
                    }
                }
                assert!(
                    start.elapsed() < timeout,
                    "timed out waiting for descendant PID at {}",
                    path.display()
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }

        fn process_is_gone_or_zombie(pid: i32) -> bool {
            if unsafe { libc::kill(pid, 0) } == -1 {
                return std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
            }
            let stat_path = format!("/proc/{pid}/stat");
            std::fs::read_to_string(stat_path)
                .ok()
                .and_then(|stat| {
                    stat.rsplit_once(") ")
                        .map(|(_, rest)| rest.starts_with('Z'))
                })
                .unwrap_or(false)
        }

        let dir = TempDir::new().expect("tempdir");
        let pid_file = dir.path().join("descendant.pid");
        let pid_path = pid_file.to_str().expect("utf-8 path").to_string();

        // Mirror run_actor_inner: std Command → process group → async-process.
        let mut std_cmd = StdCommand::new("sh");
        std_cmd
            .args([
                "-c",
                "sleep 30 & echo $! > \"$1\"; exec sleep 30",
                "_",
                &pid_path,
            ])
            .current_dir(dir.path());
        configure_process_group(&mut std_cmd);
        let mut command = async_process::Command::from(std_cmd);
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().expect("spawn stand-in agent");
        let mut cleanup = ProcessGroupCleanup::new(Some(child.id()));

        let descendant = wait_for_pid_file(&pid_file, Duration::from_secs(2)).await;

        // Same shutdown sequence as run_actor_inner after the actor loop ends.
        if let Ok(pid) = i32::try_from(child.id()) {
            crate::procutil::kill_process_group(pid);
        }
        let _ = child.kill();
        let _ = child.status().await;
        cleanup.disarm();

        let mut exited = false;
        for _ in 0..40 {
            if process_is_gone_or_zombie(descendant) {
                exited = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        if !exited {
            // SAFETY: avoid leaking a sleep if the assertion fails.
            unsafe {
                libc::kill(descendant, libc::SIGKILL);
            }
        }
        assert!(
            exited,
            "agent-spawned descendant {descendant} survived process-group kill"
        );
    }
}
