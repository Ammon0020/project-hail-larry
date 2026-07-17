//! ACP session transport and actor lifecycle.
//!
//! The SDK connection is deliberately owned by one task per session. Its
//! `connect_with` closure is the only place a `ConnectionTo<Agent>` is valid,
//! so callers communicate with that task through a bounded command channel
//! rather than attempting to store an SDK connection in the session registry.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, RwLock};

use agent_client_protocol::schema::v1::{
    CancelNotification, ClientCapabilities, ContentBlock, CreateTerminalRequest,
    CreateTerminalResponse, FileSystemCapabilities, InitializeRequest, KillTerminalRequest,
    KillTerminalResponse, NewSessionRequest, PromptRequest, ReadTextFileRequest,
    ReadTextFileResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionId, SessionNotification, TerminalExitStatus,
    TerminalOutputRequest, TerminalOutputResponse, TextContent, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Agent, ByteStreams, Client as SdkClient, ConnectionTo};
use async_process::Command;
use async_trait::async_trait;
use chrono::Utc;
use futures_util::io::AsyncReadExt;
use tokio::sync::{mpsc, oneshot, watch, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::AgentRegistry;
use crate::config::AgentInfo;
use crate::interfaces::{
    ACPClient, AppError, Attachment, PermissionManager, Session, SessionInfo, WorkspaceManager,
};
use crate::shell::{merge_env, Executor, DEFAULT_MAX_OUTPUT_BYTES};

/// Maximum retained agent stderr diagnostic tail. Agent stderr is untrusted and
/// must never be allowed to grow the daemon's memory without bound.
pub const STDERR_TAIL_BYTES: usize = 8 * 1024;
const ACTOR_COMMAND_CAPACITY: usize = 32;
/// Maximum callback requests an agent can make concurrently per session.
const MAX_CALLBACK_TASKS: usize = 16;
/// Maximum terminal records retained per ACP session.
const MAX_TERMINALS_PER_SESSION: usize = 16;
/// Maximum output retained for an ACP terminal when the agent gives no lower limit.
const MAX_TERMINAL_OUTPUT_BYTES: usize = DEFAULT_MAX_OUTPUT_BYTES;

/// Constructor-only dependencies for ACP core.
pub struct ClientDeps {
    pub registry: Arc<AgentRegistry>,
    pub workspaces: Arc<dyn WorkspaceManager>,
    pub permissions: Arc<dyn PermissionManager>,
}

/// Session status stored in the in-memory registry during the core port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Created,
    Running,
    Idle,
    Interrupted,
    Failed,
    Closed,
}

impl SessionState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Idle => "idle",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Closed => "closed",
        }
    }
}

struct SessionEntry {
    info: SessionInfo,
    state: SessionState,
    commands: mpsc::Sender<ActorCommand>,
    stderr_tail: Arc<Mutex<StderrTail>>,
}

/// ACP lifecycle service. The registry lock protects only metadata and command
/// senders; every async command is sent after cloning its sender.
pub struct Client {
    deps: ClientDeps,
    sessions: Arc<RwLock<HashMap<String, SessionEntry>>>,
}

impl Client {
    /// Creates an ACP client with all service dependencies supplied up front.
    #[must_use]
    pub fn new(deps: ClientDeps) -> Self {
        Self {
            deps,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn session_sender(&self, session_id: &str) -> Result<mpsc::Sender<ActorCommand>, AppError> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))?;
        let entry = sessions
            .get(session_id)
            .ok_or_else(|| AppError::not_found("session"))?;
        match entry.state {
            SessionState::Failed => Err(AppError::internal(
                "ACP session failed; close it and create a new session",
            )),
            SessionState::Closed => Err(AppError::internal("ACP session is closed")),
            _ => Ok(entry.commands.clone()),
        }
    }

    /// Reserve the session's sole prompt slot before enqueuing the actor command.
    fn begin_prompt(&self, session_id: &str) -> Result<mpsc::Sender<ActorCommand>, AppError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))?;
        let entry = sessions
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found("session"))?;
        match entry.state {
            SessionState::Idle | SessionState::Interrupted => {
                entry.state = SessionState::Running;
                entry.info.status = SessionState::Running.as_str().to_string();
                entry.info.updated_at = Utc::now();
                Ok(entry.commands.clone())
            }
            SessionState::Running => Err(AppError::validation(
                "ACP session already has an active prompt",
            )),
            SessionState::Failed => Err(AppError::internal(
                "ACP session failed; close it and create a new session",
            )),
            SessionState::Closed | SessionState::Created => {
                Err(AppError::internal("ACP session is not ready for prompts"))
            }
        }
    }

    fn update_state(&self, session_id: &str, state: SessionState) {
        if let Ok(mut sessions) = self.sessions.write() {
            if let Some(entry) = sessions.get_mut(session_id) {
                entry.state = state;
                entry.info.status = state.as_str().to_string();
                entry.info.updated_at = Utc::now();
            }
        }
    }

    /// Move a session only when no concurrent lifecycle operation superseded it.
    fn update_state_if(&self, session_id: &str, expected: SessionState, state: SessionState) {
        if let Ok(mut sessions) = self.sessions.write() {
            if let Some(entry) = sessions.get_mut(session_id) {
                if entry.state == expected {
                    entry.state = state;
                    entry.info.status = state.as_str().to_string();
                    entry.info.updated_at = Utc::now();
                }
            }
        }
    }

    /// Return the retained, bounded stderr tail for a session.
    pub fn stderr_tail(&self, session_id: &str) -> Result<String, AppError> {
        let tail = self
            .sessions
            .read()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))?
            .get(session_id)
            .map(|entry| Arc::clone(&entry.stderr_tail))
            .ok_or_else(|| AppError::not_found("session"))?;
        let stderr = tail
            .lock()
            .map_err(|_| AppError::internal("ACP stderr lock poisoned"))?
            .as_string();
        Ok(stderr)
    }
}

#[async_trait]
impl ACPClient for Client {
    async fn list_agents(&self) -> Result<Vec<AgentInfo>, AppError> {
        Ok(self.deps.registry.list())
    }

    fn register_agent(&self, agent: AgentInfo) {
        self.deps.registry.register(agent);
    }

    fn remove_agent(&self, id: &str) {
        self.deps.registry.remove(id);
    }

    async fn create_session(
        &self,
        agent_id: &str,
        model_id: &str,
        workspace_id: &str,
    ) -> Result<SessionInfo, AppError> {
        let agent = self
            .deps
            .registry
            .resolve(agent_id, model_id)
            .map_err(AppError::validation)?;
        let workspace = self
            .deps
            .workspaces
            .list()
            .await?
            .into_iter()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(|| AppError::not_found("workspace"))?;
        let workspace_path = PathBuf::from(workspace.path);
        let id = format!("sess-{}", Uuid::new_v4().simple());
        let now = Utc::now();
        let info = SessionInfo {
            id: id.clone(),
            name: "New chat".to_string(),
            status: SessionState::Created.as_str().to_string(),
            agent_id: agent_id.to_string(),
            model_id: model_id.to_string(),
            workspace: workspace_id.to_string(),
            created_at: now,
            updated_at: now,
        };
        let (commands, receiver) = mpsc::channel(ACTOR_COMMAND_CAPACITY);
        let (ready_tx, ready_rx) = oneshot::channel();
        let (registered_tx, registered_rx) = oneshot::channel();
        let stderr_tail = Arc::new(Mutex::new(StderrTail::default()));
        let actor = ActorConfig {
            local_session_id: id.clone(),
            agent,
            workspace_id: workspace_id.to_string(),
            workspace_path,
            permissions: Arc::clone(&self.deps.permissions),
            workspaces: Arc::clone(&self.deps.workspaces),
            stderr_tail: Arc::clone(&stderr_tail),
            sessions: Arc::clone(&self.sessions),
        };
        tokio::spawn(run_actor(actor, receiver, ready_tx, registered_rx));

        // Do not publish a session until its agent has initialized and supplied
        // an ACP session ID. A dead-on-arrival child is reported with its tail.
        let result = ready_rx
            .await
            .map_err(|_| AppError::internal("ACP session actor exited during startup"))?;
        result?;
        self.sessions
            .write()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))?
            .insert(
                id,
                SessionEntry {
                    info: info.clone(),
                    state: SessionState::Idle,
                    commands,
                    stderr_tail,
                },
            );
        // The actor waits for this handoff before accepting commands. That
        // eliminates the race where a connection could exit just after startup
        // but before its owning local session entry was registered.
        let _ = registered_tx.send(());
        Ok(info)
    }

    fn get_session_info(&self, session_id: &str) -> Result<SessionInfo, AppError> {
        self.sessions
            .read()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))?
            .get(session_id)
            .map(|entry| entry.info.clone())
            .ok_or_else(|| AppError::not_found("session"))
    }

    fn list_sessions(&self) -> Vec<Session> {
        let Ok(sessions) = self.sessions.read() else {
            return Vec::new();
        };
        let mut values: Vec<_> = sessions.values().map(|entry| entry.info.clone()).collect();
        values.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        values
    }

    async fn send_prompt(
        &self,
        session_id: &str,
        content: &str,
        _attachments: &[Attachment],
    ) -> Result<(), AppError> {
        let sender = self.begin_prompt(session_id)?;
        let (result_tx, result_rx) = oneshot::channel();
        sender
            .send(ActorCommand::Prompt {
                content: content.to_string(),
                result: result_tx,
            })
            .await
            .map_err(|_| AppError::internal("ACP session actor is unavailable"))?;
        result_rx
            .await
            .map_err(|_| AppError::internal("ACP prompt actor exited"))??;
        // Cancellation can arrive while the prompt RPC is in flight. Do not
        // overwrite its Interrupted state after the RPC's response arrives.
        self.update_state_if(session_id, SessionState::Running, SessionState::Idle);
        Ok(())
    }

    fn rename_session(&self, session_id: &str, name: &str) -> Result<(), AppError> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))?;
        let entry = sessions
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found("session"))?;
        entry.info.name = name.to_string();
        entry.info.updated_at = Utc::now();
        Ok(())
    }

    async fn rebind_session(
        &self,
        _session_id: &str,
        _agent_id: &str,
        _model_id: &str,
        _max_transfer_bytes: i64,
    ) -> Result<SessionInfo, AppError> {
        Err(AppError::unsupported(
            "session rebind is implemented by S-ACP-CONTEXT",
        ))
    }

    async fn switch_model(&self, _session_id: &str, _model_id: &str) -> Result<(), AppError> {
        Err(AppError::unsupported(
            "model switching is implemented by S-ACP-PROVIDERS",
        ))
    }

    async fn cancel_session(&self, session_id: &str) -> Result<(), AppError> {
        let sender = self.session_sender(session_id)?;
        sender
            .send(ActorCommand::Cancel)
            .await
            .map_err(|_| AppError::internal("ACP session actor is unavailable"))?;
        self.update_state(session_id, SessionState::Interrupted);
        Ok(())
    }

    async fn close_session(&self, session_id: &str) -> Result<(), AppError> {
        // Removing first makes close idempotent from the public registry's
        // perspective and prevents new work from being queued while teardown
        // is in progress. The actor still owns the sender copied below.
        let entry = self
            .sessions
            .write()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))?
            .remove(session_id)
            .ok_or_else(|| AppError::not_found("session"))?;
        self.deps.permissions.clear_session(session_id);
        let (closed_tx, closed_rx) = oneshot::channel();
        if entry
            .commands
            .send(ActorCommand::Close(closed_tx))
            .await
            .is_ok()
        {
            // The actor acknowledges only after it has dropped the SDK
            // connection and reaped its child, so public close never returns
            // while an owned agent process remains alive.
            let _ = closed_rx.await;
        }
        Ok(())
    }

    fn set_session_profile(&self, _session_id: &str, _profile: &str) {}

    async fn list_providers(
        &self,
        _session_id: &str,
    ) -> Result<Vec<crate::interfaces::ProviderInfo>, AppError> {
        Err(AppError::unsupported(
            "providers are implemented by S-ACP-PROVIDERS",
        ))
    }

    async fn set_provider(
        &self,
        _session_id: &str,
        _id: &str,
        _api_type: &str,
        _base_url: &str,
        _headers: std::collections::HashMap<String, String>,
    ) -> Result<(), AppError> {
        Err(AppError::unsupported(
            "providers are implemented by S-ACP-PROVIDERS",
        ))
    }

    async fn disable_provider(&self, _session_id: &str, _id: &str) -> Result<(), AppError> {
        Err(AppError::unsupported(
            "providers are implemented by S-ACP-PROVIDERS",
        ))
    }
}

enum ActorCommand {
    Prompt {
        content: String,
        result: oneshot::Sender<Result<(), AppError>>,
    },
    Cancel,
    Close(oneshot::Sender<()>),
}

struct ActorConfig {
    local_session_id: String,
    agent: AgentInfo,
    workspace_id: String,
    workspace_path: PathBuf,
    permissions: Arc<dyn PermissionManager>,
    workspaces: Arc<dyn WorkspaceManager>,
    stderr_tail: Arc<Mutex<StderrTail>>,
    sessions: Arc<RwLock<HashMap<String, SessionEntry>>>,
}

async fn run_actor(
    config: ActorConfig,
    mut commands: mpsc::Receiver<ActorCommand>,
    ready: oneshot::Sender<Result<(), AppError>>,
    registered: oneshot::Receiver<()>,
) {
    let mut ready = Some(ready);
    let mut registered = Some(registered);
    let result = run_actor_inner(&config, &mut commands, &mut ready, &mut registered).await;
    match result {
        Ok(ActorExit::Closed(close_results)) => {
            for result in close_results {
                let _ = result.send(());
            }
        }
        Err(error) => {
            if let Some(ready) = ready.take() {
                let _ = ready.send(Err(startup_error(&error, &config.stderr_tail)));
            }
            fail_session(&config);
            tracing::warn!(error = %error, "ACP session actor ended");
        }
    }
}

/// Mark a live session failed and clear permission state after actor loss.
fn fail_session(config: &ActorConfig) {
    if let Ok(mut sessions) = config.sessions.write() {
        if let Some(entry) = sessions.get_mut(&config.local_session_id) {
            entry.state = SessionState::Failed;
            entry.info.status = SessionState::Failed.as_str().to_string();
            entry.info.updated_at = Utc::now();
        }
    }
    config.permissions.clear_session(&config.local_session_id);
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

async fn run_actor_inner(
    config: &ActorConfig,
    commands: &mut mpsc::Receiver<ActorCommand>,
    ready: &mut Option<oneshot::Sender<Result<(), AppError>>>,
    registered: &mut Option<oneshot::Receiver<()>>,
) -> Result<ActorExit, AppError> {
    let mut command = Command::new(&config.agent.command);
    command
        .args(&config.agent.args)
        .current_dir(&config.workspace_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| AppError::internal(format!("spawn ACP agent: {error}")))?;
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
    let terminals = Arc::new(Mutex::new(HashMap::new()));
    let handler_cancel = CancellationToken::new();
    let read_deps = HandlerDeps {
        local_session_id: config.local_session_id.clone(),
        workspace_id: config.workspace_id.clone(),
        workspace_path: config.workspace_path.clone(),
        workspaces: Arc::clone(&config.workspaces),
        permissions: Arc::clone(&config.permissions),
        terminals: Arc::clone(&terminals),
        cancellation: handler_cancel.clone(),
        callback_slots: Arc::new(Semaphore::new(MAX_CALLBACK_TASKS)),
    };
    let write_deps = read_deps.clone();
    let permission_deps = read_deps.clone();
    let terminal_deps = read_deps.clone();
    let terminal_output_deps = read_deps.clone();
    let terminal_wait_deps = read_deps.clone();
    let terminal_kill_deps = read_deps.clone();
    let terminal_release_deps = read_deps.clone();
    let connected = SdkClient
        .builder()
        .name("local-agent")
        .on_receive_notification(
            async |_notification: SessionNotification, _cx: ConnectionTo<Agent>| Ok(()),
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: ReadTextFileRequest, responder, _cx: ConnectionTo<Agent>| {
                let deps = read_deps.clone();
                let Some(permit) = callback_permit(&deps) else {
                    let _ = responder.respond_with_internal_error(callback_limit_error());
                    return Ok(());
                };
                spawn_callback(deps.cancellation.clone(), permit, async move {
                    match read_text_file(deps, request).await {
                        Ok(response) => {
                            let _ = responder.respond(response);
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "ACP denied file read");
                            let _ = responder.respond_with_internal_error(error);
                        }
                    }
                });
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WriteTextFileRequest, responder, _cx: ConnectionTo<Agent>| {
                let deps = write_deps.clone();
                let Some(permit) = callback_permit(&deps) else {
                    let _ = responder.respond_with_internal_error(callback_limit_error());
                    return Ok(());
                };
                spawn_callback(deps.cancellation.clone(), permit, async move {
                    match write_text_file(deps, request).await {
                        Ok(response) => {
                            let _ = responder.respond(response);
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "ACP denied file write");
                            let _ = responder.respond_with_internal_error(error);
                        }
                    }
                });
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx: ConnectionTo<Agent>| {
                let deps = permission_deps.clone();
                // The permission manager waits for a user device. It must not
                // block the SDK dispatch task, or the agent cannot process
                // cancellation and unrelated callbacks while waiting.
                let Some(permit) = callback_permit(&deps) else {
                    let _ = responder.respond_with_internal_error(callback_limit_error());
                    return Ok(());
                };
                spawn_callback(deps.cancellation.clone(), permit, async move {
                    let response = request_permission(deps, request).await;
                    let _ = responder.respond(response);
                });
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CreateTerminalRequest, responder, _cx: ConnectionTo<Agent>| {
                let deps = terminal_deps.clone();
                let Some(permit) = callback_permit(&deps) else {
                    let _ = responder.respond_with_internal_error(callback_limit_error());
                    return Ok(());
                };
                spawn_callback(deps.cancellation.clone(), permit, async move {
                    match create_terminal(deps, request) {
                        Ok(response) => {
                            let _ = responder.respond(response);
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "ACP denied terminal create");
                            let _ = responder.respond_with_internal_error(error);
                        }
                    }
                });
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: TerminalOutputRequest, responder, _cx: ConnectionTo<Agent>| {
                let deps = terminal_output_deps.clone();
                let Some(permit) = callback_permit(&deps) else {
                    let _ = responder.respond_with_internal_error(callback_limit_error());
                    return Ok(());
                };
                spawn_callback(deps.cancellation.clone(), permit, async move {
                    match terminal_output(deps, request) {
                        Ok(response) => {
                            let _ = responder.respond(response);
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "ACP terminal output failed");
                            let _ = responder.respond_with_internal_error(error);
                        }
                    }
                });
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WaitForTerminalExitRequest,
                        responder,
                        _cx: ConnectionTo<Agent>| {
                let deps = terminal_wait_deps.clone();
                // A terminal can run indefinitely. Waiting in this task would
                // stop the SDK from dispatching cancellation and other callbacks.
                let Some(permit) = callback_permit(&deps) else {
                    let _ = responder.respond_with_internal_error(callback_limit_error());
                    return Ok(());
                };
                spawn_callback(deps.cancellation.clone(), permit, async move {
                    match wait_for_terminal_exit(deps, request).await {
                        Ok(response) => {
                            let _ = responder.respond(response);
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "ACP terminal wait failed");
                            let _ = responder.respond_with_internal_error(error);
                        }
                    }
                });
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: KillTerminalRequest, responder, _cx: ConnectionTo<Agent>| {
                let deps = terminal_kill_deps.clone();
                let Some(permit) = callback_permit(&deps) else {
                    let _ = responder.respond_with_internal_error(callback_limit_error());
                    return Ok(());
                };
                spawn_callback(deps.cancellation.clone(), permit, async move {
                    match kill_terminal(deps, request) {
                        Ok(response) => {
                            let _ = responder.respond(response);
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "ACP terminal kill failed");
                            let _ = responder.respond_with_internal_error(error);
                        }
                    }
                });
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReleaseTerminalRequest, responder, _cx: ConnectionTo<Agent>| {
                let deps = terminal_release_deps.clone();
                let Some(permit) = callback_permit(&deps) else {
                    let _ = responder.respond_with_internal_error(callback_limit_error());
                    return Ok(());
                };
                spawn_callback(deps.cancellation.clone(), permit, async move {
                    match release_terminal(deps, request) {
                        Ok(response) => {
                            let _ = responder.respond(response);
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "ACP terminal release failed");
                            let _ = responder.respond_with_internal_error(error);
                        }
                    }
                });
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, |cx: ConnectionTo<Agent>| async move {
            let initialize = InitializeRequest::new(ProtocolVersion::V1).client_capabilities(
                ClientCapabilities::new().fs(FileSystemCapabilities::new()
                    .read_text_file(true)
                    .write_text_file(true)),
            );
            cx.send_request(initialize)
                .block_task()
                .await
                .map_err(|_| agent_client_protocol::Error::internal_error())?;
            let session = cx
                .send_request(NewSessionRequest::new(config.workspace_path.clone()))
                .block_task()
                .await
                .map_err(|_| agent_client_protocol::Error::internal_error())?;
            if let Some(ready) = ready.take() {
                let _ = ready.send(Ok(()));
            }
            if let Some(registered) = registered.take() {
                registered
                    .await
                    .map_err(|_| agent_client_protocol::Error::internal_error())?;
            }
            actor_loop(cx, session.session_id, commands).await
        })
        .await;
    handler_cancel.cancel();
    cancel_terminals(&terminals);
    let _ = child.kill();
    let _ = child.status().await;
    connected.map_err(|error| AppError::internal(format!("ACP connection: {error}")))
}

/// Outcome returned by the connection-owning actor loop.
enum ActorExit {
    Closed(Vec<oneshot::Sender<()>>),
}

async fn actor_loop(
    cx: ConnectionTo<Agent>,
    agent_session_id: SessionId,
    commands: &mut mpsc::Receiver<ActorCommand>,
) -> Result<ActorExit, agent_client_protocol::Error> {
    while let Some(command) = commands.recv().await {
        match command {
            ActorCommand::Prompt { content, result } => {
                match await_prompt(
                    cx.clone(),
                    agent_session_id.clone(),
                    content,
                    result,
                    commands,
                )
                .await?
                {
                    PromptExit::Continue => {}
                    PromptExit::Closed(results) => return Ok(ActorExit::Closed(results)),
                }
            }
            ActorCommand::Cancel => {
                cx.send_notification(CancelNotification::new(agent_session_id.clone()))
                    .map_err(|_| agent_client_protocol::Error::internal_error())?;
            }
            ActorCommand::Close(result) => {
                return Ok(ActorExit::Closed(vec![result]));
            }
        }
    }
    Err(agent_client_protocol::Error::internal_error())
}

enum PromptExit {
    Continue,
    Closed(Vec<oneshot::Sender<()>>),
}

/// Await one prompt while continuing to receive session control commands.
async fn await_prompt(
    cx: ConnectionTo<Agent>,
    agent_session_id: SessionId,
    content: String,
    result: oneshot::Sender<Result<(), AppError>>,
    commands: &mut mpsc::Receiver<ActorCommand>,
) -> Result<PromptExit, agent_client_protocol::Error> {
    let prompt = cx
        .send_request(PromptRequest::new(
            agent_session_id.clone(),
            vec![ContentBlock::Text(TextContent::new(content))],
        ))
        .block_task();
    tokio::pin!(prompt);
    let mut result = Some(result);

    loop {
        tokio::select! {
            reply = &mut prompt => {
                if let Some(result) = result.take() {
                    let reply = reply
                        .map(|_| ())
                        .map_err(|error| AppError::internal(format!("ACP prompt: {error}")));
                    let _ = result.send(reply);
                }
                return Ok(PromptExit::Continue);
            }
            command = commands.recv() => {
                match command {
                    Some(ActorCommand::Cancel) => {
                        let cancel = cx
                            .send_notification(CancelNotification::new(agent_session_id.clone()))
                            .map_err(|_| agent_client_protocol::Error::internal_error());
                        if let Some(result) = result.take() {
                            let _ = result.send(Err(AppError::internal("ACP prompt cancelled")));
                        }
                        cancel?;
                        return Ok(PromptExit::Continue);
                    }
                    Some(ActorCommand::Close(close)) => {
                        if let Some(result) = result.take() {
                            let _ = result.send(Err(AppError::internal("ACP session closed during prompt")));
                        }
                        return Ok(PromptExit::Closed(vec![close]));
                    }
                    Some(ActorCommand::Prompt { result, .. }) => {
                        let _ = result.send(Err(AppError::validation(
                            "ACP session already has an active prompt",
                        )));
                    }
                    None => return Err(agent_client_protocol::Error::internal_error()),
                }
            }
        }
    }
}

#[derive(Clone)]
struct HandlerDeps {
    local_session_id: String,
    workspace_id: String,
    workspace_path: PathBuf,
    workspaces: Arc<dyn WorkspaceManager>,
    permissions: Arc<dyn PermissionManager>,
    terminals: TerminalRegistry,
    cancellation: CancellationToken,
    callback_slots: Arc<Semaphore>,
}

/// Reserve one bounded callback worker without blocking SDK request dispatch.
fn callback_permit(deps: &HandlerDeps) -> Option<OwnedSemaphorePermit> {
    deps.callback_slots.clone().try_acquire_owned().ok()
}

/// Run a callback until it completes or its owning ACP session closes.
fn spawn_callback<F>(cancellation: CancellationToken, permit: OwnedSemaphorePermit, future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let _permit = permit;
        tokio::select! {
            () = cancellation.cancelled() => {}
            () = future => {}
        }
    });
}

fn callback_limit_error() -> AppError {
    AppError::internal("ACP callback capacity exceeded")
}

type TerminalRegistry = Arc<Mutex<HashMap<String, Arc<TerminalState>>>>;

/// State shared by callback requests for one ACP terminal.
///
/// The standard mutex is intentionally used only for short, synchronous state
/// updates. Terminal waits observe the watch channel outside the lock.
struct TerminalState {
    cancel: CancellationToken,
    output: Mutex<RetainedOutput>,
    exit: watch::Sender<Option<TerminalExitStatus>>,
}

/// Bounded terminal output that discards the oldest complete UTF-8 prefix.
struct RetainedOutput {
    text: String,
    limit: usize,
    truncated: bool,
}

impl RetainedOutput {
    fn new(limit: usize) -> Self {
        Self {
            text: String::new(),
            limit,
            truncated: false,
        }
    }

    fn push_line(&mut self, line: &str) {
        if self.limit == 0 {
            // Each callback invocation represents at least a newline, even
            // when the emitted line is empty.
            self.truncated = true;
            return;
        }
        self.text.push_str(line);
        self.text.push('\n');
        if self.text.len() > self.limit {
            let excess = self.text.len() - self.limit;
            let start = self.text.ceil_char_boundary(excess);
            self.text.drain(..start);
            self.truncated = true;
        }
    }
}

/// Create an ACP terminal and start its command without delaying the response.
fn create_terminal(
    deps: HandlerDeps,
    request: CreateTerminalRequest,
) -> Result<CreateTerminalResponse, AppError> {
    if deps.cancellation.is_cancelled() {
        return Err(AppError::internal("ACP session is closing"));
    }
    let cwd = terminal_cwd(&deps.workspace_path, request.cwd.as_deref())?;
    let limit = request
        .output_byte_limit
        .map_or(MAX_TERMINAL_OUTPUT_BYTES, |limit| {
            usize::try_from(limit)
                .unwrap_or(MAX_TERMINAL_OUTPUT_BYTES)
                .min(MAX_TERMINAL_OUTPUT_BYTES)
        });
    let terminal_id = format!("term-{}", Uuid::new_v4().simple());
    let (exit, _) = watch::channel(None);
    let state = Arc::new(TerminalState {
        cancel: deps.cancellation.child_token(),
        output: Mutex::new(RetainedOutput::new(limit)),
        exit,
    });
    {
        let mut terminals = deps
            .terminals
            .lock()
            .map_err(|_| AppError::internal("ACP terminal registry lock poisoned"))?;
        if terminals.len() >= MAX_TERMINALS_PER_SESSION {
            return Err(AppError::internal("ACP terminal capacity exceeded"));
        }
        terminals.insert(terminal_id.clone(), Arc::clone(&state));
    }

    let env = merge_env(
        std::env::vars(),
        request
            .env
            .iter()
            .map(|variable| (variable.name.clone(), variable.value.clone())),
    );
    let command = request.command;
    let args = request.args;
    let executor = Executor::new(&deps.workspace_path)
        .with_env(env)
        .with_max_output_bytes(limit);
    tokio::spawn(async move {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let stdout_state = Arc::clone(&state);
        let stderr_state = Arc::clone(&state);
        let (result, error) = executor
            .run_async_args(
                state.cancel.clone(),
                &command,
                &args,
                cwd.as_deref(),
                move |line| append_terminal_output(&stdout_state, line),
                move |line| append_terminal_output(&stderr_state, line),
            )
            .await;
        if let Some(error) = error {
            // Commands, argv, and environment values may contain credentials;
            // keep the diagnostic category without recording their contents.
            tracing::warn!(error = %error, "ACP terminal command ended abnormally");
        }
        let status = TerminalExitStatus::new()
            .exit_code((result.exit_code >= 0).then_some(result.exit_code as u32))
            .signal(result.signal);
        state.exit.send_replace(Some(status));
    });
    Ok(CreateTerminalResponse::new(terminal_id))
}

/// Return a snapshot of terminal output without waiting for the command.
fn terminal_output(
    deps: HandlerDeps,
    request: TerminalOutputRequest,
) -> Result<TerminalOutputResponse, AppError> {
    let terminal = terminal_state(&deps.terminals, &request.terminal_id.to_string())?;
    let output = terminal
        .output
        .lock()
        .map_err(|_| AppError::internal("ACP terminal output lock poisoned"))?;
    let exit_status = terminal.exit.borrow().clone();
    Ok(TerminalOutputResponse::new(output.text.clone(), output.truncated).exit_status(exit_status))
}

/// Wait asynchronously for an owned terminal to exit.
async fn wait_for_terminal_exit(
    deps: HandlerDeps,
    request: WaitForTerminalExitRequest,
) -> Result<WaitForTerminalExitResponse, AppError> {
    let terminal = terminal_state(&deps.terminals, &request.terminal_id.to_string())?;
    let mut exit = terminal.exit.subscribe();
    loop {
        if let Some(status) = exit.borrow().clone() {
            return Ok(WaitForTerminalExitResponse::new(status));
        }
        exit.changed()
            .await
            .map_err(|_| AppError::internal("ACP terminal exited without a status"))?;
    }
}

/// Cancel a terminal while retaining its output for subsequent inspection.
fn kill_terminal(
    deps: HandlerDeps,
    request: KillTerminalRequest,
) -> Result<KillTerminalResponse, AppError> {
    let terminal = terminal_state(&deps.terminals, &request.terminal_id.to_string())?;
    terminal.cancel.cancel();
    Ok(KillTerminalResponse::new())
}

/// Cancel and remove a terminal, releasing its registry-owned resources.
fn release_terminal(
    deps: HandlerDeps,
    request: ReleaseTerminalRequest,
) -> Result<ReleaseTerminalResponse, AppError> {
    let terminal = deps
        .terminals
        .lock()
        .map_err(|_| AppError::internal("ACP terminal registry lock poisoned"))?
        .remove(&request.terminal_id.to_string())
        .ok_or_else(|| AppError::not_found("terminal"))?;
    terminal.cancel.cancel();
    Ok(ReleaseTerminalResponse::new())
}

fn terminal_state(
    registry: &TerminalRegistry,
    terminal_id: &str,
) -> Result<Arc<TerminalState>, AppError> {
    registry
        .lock()
        .map_err(|_| AppError::internal("ACP terminal registry lock poisoned"))?
        .get(terminal_id)
        .cloned()
        .ok_or_else(|| AppError::not_found("terminal"))
}

fn append_terminal_output(state: &TerminalState, line: &str) {
    if let Ok(mut output) = state.output.lock() {
        output.push_line(line);
    }
}

/// Cancel every terminal when its owning ACP session disconnects.
fn cancel_terminals(registry: &TerminalRegistry) {
    if let Ok(mut terminals) = registry.lock() {
        for terminal in terminals.values() {
            terminal.cancel.cancel();
        }
        terminals.clear();
    }
}

/// Validate the ACP-required absolute CWD and translate it for Executor.
fn terminal_cwd(root: &Path, cwd: Option<&Path>) -> Result<Option<String>, AppError> {
    let Some(cwd) = cwd else {
        return Ok(None);
    };
    if !cwd.is_absolute() {
        return Err(AppError::validation(
            "terminal cwd must be an absolute path within the workspace",
        ));
    }
    let root = std::fs::canonicalize(root)
        .map_err(|error| AppError::internal(format!("canonicalize workspace: {error}")))?;
    let cwd = std::fs::canonicalize(cwd)
        .map_err(|error| AppError::validation(format!("invalid terminal cwd: {error}")))?;
    if !cwd.is_dir() {
        return Err(AppError::validation("terminal cwd is not a directory"));
    }
    let relative = cwd
        .strip_prefix(&root)
        .map_err(|_| AppError::validation("terminal cwd is outside the workspace"))?;
    if relative.as_os_str().is_empty() {
        Ok(None)
    } else {
        relative
            .to_str()
            .map(|path| Some(path.to_string()))
            .ok_or_else(|| AppError::validation("terminal cwd is not valid Unicode"))
    }
}

async fn read_text_file(
    deps: HandlerDeps,
    request: ReadTextFileRequest,
) -> Result<ReadTextFileResponse, AppError> {
    let content = match workspace_relative_path(&deps.workspace_path, &request.path).await {
        Ok(path) => deps
            .workspaces
            .read_file(&deps.workspace_id, &path)
            .await
            .map(|result| result.content),
        Err(error) => Err(error),
    };
    content.map(ReadTextFileResponse::new)
}

async fn write_text_file(
    deps: HandlerDeps,
    request: WriteTextFileRequest,
) -> Result<WriteTextFileResponse, AppError> {
    let result = match workspace_relative_path(&deps.workspace_path, &request.path).await {
        Ok(path) => deps
            .workspaces
            .write_file(&deps.workspace_id, &path, &request.content, 0)
            .await
            .map(|_| ()),
        Err(error) => Err(error),
    };
    result.map(|()| WriteTextFileResponse::new())
}

async fn request_permission(
    deps: HandlerDeps,
    request: RequestPermissionRequest,
) -> RequestPermissionResponse {
    let tool = request
        .tool_call
        .fields
        .title
        .clone()
        .unwrap_or_else(|| "Tool call".to_string());
    let tool_kind = request
        .tool_call
        .fields
        .kind
        .as_ref()
        .map_or_else(String::new, tool_kind_name);
    let command = request
        .tool_call
        .fields
        .raw_input
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    let target = request
        .tool_call
        .fields
        .locations
        .as_ref()
        .and_then(|locations| locations.first())
        .map_or_else(String::new, |location| {
            location.path.to_string_lossy().into_owned()
        });
    let options = request
        .options
        .iter()
        .filter_map(|option| permission_decision(&option.kind))
        .collect();
    let option_details = request
        .options
        .iter()
        .map(|option| crate::interfaces::PermissionOptionInfo {
            id: option.option_id.to_string(),
            name: option.name.clone(),
            kind: permission_kind_name(&option.kind).to_string(),
        })
        .collect();
    let permission = crate::interfaces::PermissionRequest {
        id: Uuid::new_v4().to_string(),
        // Agent session IDs are protocol transport identifiers. Permissions
        // belong to the local lifecycle entry so close clears its exact
        // pending prompts and durable policies.
        session_id: deps.local_session_id.clone(),
        tool,
        tool_kind,
        command,
        target,
        options,
        option_details,
    };
    match deps.permissions.request(permission).await {
        Ok(decision) => request
            .options
            .iter()
            .find(|option| permission_decision(&option.kind) == Some(decision))
            .map_or_else(
                || RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled),
                |option| {
                    RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
                        SelectedPermissionOutcome::new(option.option_id.clone()),
                    ))
                },
            ),
        Err(error) => {
            tracing::warn!(error = %error, "ACP permission request cancelled");
            RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
        }
    }
}

fn tool_kind_name(kind: &agent_client_protocol::schema::v1::ToolKind) -> String {
    use agent_client_protocol::schema::v1::ToolKind;

    match kind {
        ToolKind::Read => "read",
        ToolKind::Edit => "edit",
        ToolKind::Delete => "delete",
        ToolKind::Move => "move",
        ToolKind::Search => "search",
        ToolKind::Execute => "execute",
        ToolKind::Think => "think",
        ToolKind::Fetch => "fetch",
        ToolKind::SwitchMode => "switch_mode",
        ToolKind::Other => "other",
        _ => "other",
    }
    .to_string()
}

fn permission_kind_name(
    kind: &agent_client_protocol::schema::v1::PermissionOptionKind,
) -> &'static str {
    use agent_client_protocol::schema::v1::PermissionOptionKind;

    match kind {
        PermissionOptionKind::AllowOnce => "allow_once",
        PermissionOptionKind::AllowAlways => "allow_always",
        PermissionOptionKind::RejectOnce => "reject_once",
        PermissionOptionKind::RejectAlways => "reject_always",
        _ => "unknown",
    }
}

fn permission_decision(
    kind: &agent_client_protocol::schema::v1::PermissionOptionKind,
) -> Option<crate::interfaces::PermissionDecision> {
    use crate::interfaces::PermissionDecision;
    use agent_client_protocol::schema::v1::PermissionOptionKind;

    match kind {
        PermissionOptionKind::AllowOnce => Some(PermissionDecision::AllowOnce),
        PermissionOptionKind::AllowAlways => Some(PermissionDecision::AllowAlways),
        PermissionOptionKind::RejectOnce => Some(PermissionDecision::Deny),
        PermissionOptionKind::RejectAlways => Some(PermissionDecision::RejectAlways),
        _ => None,
    }
}

async fn workspace_relative_path(root: &Path, path: &Path) -> Result<String, AppError> {
    if path.is_absolute() {
        path_to_workspace_relative(root, path)
    } else {
        Ok(path.to_string_lossy().into_owned())
    }
}

fn spawn_stderr_drain<R>(mut stderr: R, tail: Arc<Mutex<StderrTail>>)
where
    R: futures_util::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0_u8; 1024];
        loop {
            match stderr.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut tail) = tail.lock() {
                        tail.push(&buffer[..read]);
                    }
                }
            }
        }
    });
}

#[derive(Default)]
struct StderrTail {
    bytes: Vec<u8>,
}

impl StderrTail {
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > STDERR_TAIL_BYTES {
            let excess = self.bytes.len() - STDERR_TAIL_BYTES;
            self.bytes.drain(..excess);
        }
    }

    fn as_string(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    /// Return a bounded startup diagnostic without obvious credential-bearing lines.
    fn safe_diagnostic(&self) -> String {
        self.as_string()
            .lines()
            .filter(|line| {
                let line = line.to_ascii_lowercase();
                !line.contains("token") && !line.contains("password") && !line.contains("api_key")
            })
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(STDERR_TAIL_BYTES)
            .collect::<String>()
            .trim()
            .to_string()
    }
}

#[allow(dead_code)]
fn path_to_workspace_relative(root: &Path, path: &Path) -> Result<String, AppError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| AppError::validation("agent path is outside the workspace"))?;
    Ok(relative.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use tempfile::TempDir;

    use super::{AgentRegistry, Client, ClientDeps, RetainedOutput};
    use crate::config::{AgentInfo, AgentModel};
    use crate::interfaces::{
        ACPClient, AppError, PermissionDecision, PermissionManager, PermissionRequest,
        WorkspaceManager,
    };
    use crate::workspace::Manager as WorkspaceRegistry;

    const MOCKAGENT_BIN: &str = "/tmp/mockagent";

    /// Records only session cleanup so the test can prove the local ID is used.
    #[derive(Default)]
    struct RecordingPermissions {
        cleared_sessions: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl PermissionManager for RecordingPermissions {
        async fn request(
            &self,
            _request: PermissionRequest,
        ) -> Result<PermissionDecision, AppError> {
            Ok(PermissionDecision::Deny)
        }

        async fn respond(
            &self,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn clear_session(&self, session_id: &str) {
            if let Ok(mut cleared) = self.cleared_sessions.lock() {
                cleared.push(session_id.to_string());
            }
        }

        fn get_pending(&self) -> Vec<PermissionRequest> {
            Vec::new()
        }
    }

    /// Create an isolated local ACP client backed by the deterministic Go fixture.
    async fn mock_client() -> (Arc<Client>, Arc<RecordingPermissions>, TempDir) {
        assert!(
            Path::new(MOCKAGENT_BIN).exists(),
            "mockagent binary missing at {MOCKAGENT_BIN}; build it with `go build -o /tmp/mockagent ./cmd/mockagent/`"
        );
        let tempdir = TempDir::new().expect("temporary workspace");
        let workspaces = Arc::new(WorkspaceRegistry::new());
        let workspace = workspaces
            .register(tempdir.path().to_str().expect("UTF-8 temporary workspace"))
            .await
            .expect("register temporary workspace");
        let permissions = Arc::new(RecordingPermissions::default());
        let registry = Arc::new(AgentRegistry::from_agents([AgentInfo {
            id: "mock".to_string(),
            name: "Mock agent".to_string(),
            command: MOCKAGENT_BIN.to_string(),
            args: Vec::new(),
            models: vec![AgentModel {
                id: "mock-model".to_string(),
                name: "Mock model".to_string(),
            }],
            warning: String::new(),
        }]));
        let client = Arc::new(Client::new(ClientDeps {
            registry,
            workspaces,
            permissions: permissions.clone(),
        }));
        let session = client
            .create_session("mock", "mock-model", &workspace.id)
            .await
            .expect("create mock ACP session");
        client
            .rename_session(&session.id, "Mock session")
            .expect("session remains registered after startup");
        (client, permissions, tempdir)
    }

    /// Wait until `send_prompt` has atomically reserved the session's turn.
    async fn wait_until_running(client: &Client, session_id: &str) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if client
                    .get_session_info(session_id)
                    .map(|session| session.status == "running")
                    .unwrap_or(false)
                {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("prompt did not reserve its session slot");
    }

    #[test]
    fn retained_terminal_output_truncates_at_utf8_boundary() {
        let mut output = RetainedOutput::new(5);
        output.push_line("éé");
        output.push_line("x");

        assert!(output.truncated);
        assert!(output.text.len() <= 5);
        assert!(std::str::from_utf8(output.text.as_bytes()).is_ok());
    }

    /// A prompt RPC must not monopolize the actor's control receiver.
    #[tokio::test]
    async fn cancel_preempts_prompt_and_rejects_second_prompt() {
        let (client, _permissions, _workspace) = mock_client().await;
        let session = client.list_sessions().pop().expect("one mock session");
        let session_id = session.id.clone();
        let prompt_client = Arc::clone(&client);
        let prompt_session_id = session_id.clone();
        let prompt = tokio::spawn(async move {
            prompt_client
                .send_prompt(&prompt_session_id, "please stream a response slowly", &[])
                .await
        });
        wait_until_running(&client, &session_id).await;

        // `send_prompt` reserves the single active slot before it enters the
        // actor channel, so this assertion has no dependency on agent IO.
        let second = client
            .send_prompt(&session_id, "concurrent prompt", &[])
            .await
            .expect_err("a second prompt must not be queued as another active turn");
        assert!(
            second.to_string().contains("active prompt"),
            "unexpected concurrent-prompt error: {second}"
        );

        tokio::time::timeout(Duration::from_secs(2), client.cancel_session(&session_id))
            .await
            .expect("cancel must be serviced while prompt is in flight")
            .expect("cancel session");
        let prompt_result = tokio::time::timeout(Duration::from_secs(2), prompt)
            .await
            .expect("cancelled prompt task must finish")
            .expect("cancelled prompt task must not panic");
        assert!(
            prompt_result.is_err(),
            "cancelled prompt unexpectedly succeeded"
        );

        client
            .close_session(&session_id)
            .await
            .expect("close after cancellation");
    }

    /// Close must interrupt a prompt, reap the actor, and clean local permission state.
    #[tokio::test]
    async fn close_preempts_prompt_and_clears_local_permission_state() {
        let (client, permissions, _workspace) = mock_client().await;
        let session = client.list_sessions().pop().expect("one mock session");
        let session_id = session.id.clone();
        let prompt_client = Arc::clone(&client);
        let prompt_session_id = session_id.clone();
        let prompt = tokio::spawn(async move {
            prompt_client
                .send_prompt(&prompt_session_id, "please stream a response slowly", &[])
                .await
        });
        wait_until_running(&client, &session_id).await;

        tokio::time::timeout(Duration::from_secs(2), client.close_session(&session_id))
            .await
            .expect("close must not wait for a prompt RPC")
            .expect("close session");
        let prompt_result = tokio::time::timeout(Duration::from_secs(2), prompt)
            .await
            .expect("closed prompt task must finish")
            .expect("closed prompt task must not panic");
        assert!(
            prompt_result.is_err(),
            "closed prompt unexpectedly succeeded"
        );
        assert!(
            client.get_session_info(&session_id).is_err(),
            "closed session must not remain callable"
        );

        let cleared = permissions
            .cleared_sessions
            .lock()
            .expect("recording permissions lock");
        assert!(
            cleared.iter().any(|id| id == &session_id),
            "close did not clear permissions using local session ID; cleared: {cleared:?}"
        );
    }
}
