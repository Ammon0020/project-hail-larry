//! ACP session transport and actor lifecycle.
//!
//! The SDK connection is deliberately owned by one task per session. Its
//! `connect_with` closure is the only place a `ConnectionTo<Agent>` is valid,
//! so callers communicate with that task through a bounded command channel
//! rather than attempting to store an SDK connection in the session registry.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
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
use agent_client_protocol::{
    Agent, ByteStreams, Client as SdkClient, ConnectionTo, JsonRpcResponse, Responder,
};
use async_process::Command;
use async_trait::async_trait;
use chrono::Utc;
use futures_util::io::AsyncReadExt;
use tokio::sync::{mpsc, oneshot, watch, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::AgentRegistry;
use crate::config::AgentInfo;
use crate::events::SharedEventBus;
use crate::interfaces::{
    wire::typed_event_to_wire, ACPClient, AppError, Attachment, EventMeta, EventPayload,
    PermissionManager, Session, SessionInfo, TypedEvent, WorkspaceManager,
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
    /// Ordered durable event stream for prompt lifecycle and ACP updates.
    pub event_bus: SharedEventBus,
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
    /// Sticky cancel bit for the reserved prompt turn. Cancel may arrive on
    /// the actor while it is still idle (Prompt not dequeued yet); the bit
    /// makes that cancel visible when the prompt eventually starts.
    prompt_cancel: Arc<AtomicBool>,
}

impl SessionEntry {
    /// Apply a lifecycle state to both the registry enum and public metadata.
    fn apply_state(&mut self, state: SessionState) {
        self.state = state;
        self.info.status = state.as_str().to_string();
        self.info.updated_at = Utc::now();
    }
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

    fn session_for_command(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<ActorCommand>, Arc<AtomicBool>), AppError> {
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
            _ => Ok((entry.commands.clone(), Arc::clone(&entry.prompt_cancel))),
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
                entry.prompt_cancel.store(false, Ordering::Release);
                entry.apply_state(SessionState::Running);
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
                entry.apply_state(state);
            }
        }
    }

    /// Move a session only when no concurrent lifecycle operation superseded it.
    fn update_state_if(&self, session_id: &str, expected: SessionState, state: SessionState) {
        if let Ok(mut sessions) = self.sessions.write() {
            if let Some(entry) = sessions.get_mut(session_id) {
                if entry.state == expected {
                    entry.apply_state(state);
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
        let prompt_cancel = Arc::new(AtomicBool::new(false));
        let actor = ActorConfig {
            local_session_id: id.clone(),
            agent,
            workspace_id: workspace_id.to_string(),
            workspace_path,
            permissions: Arc::clone(&self.deps.permissions),
            workspaces: Arc::clone(&self.deps.workspaces),
            stderr_tail: Arc::clone(&stderr_tail),
            sessions: Arc::clone(&self.sessions),
            event_bus: Arc::clone(&self.deps.event_bus),
            prompt_cancel: Arc::clone(&prompt_cancel),
        };
        tokio::spawn(run_actor(actor, receiver, ready_tx, registered_rx));

        // Do not publish a session until its agent has initialized and supplied
        // an ACP session ID. A dead-on-arrival child is reported with its tail.
        let result = ready_rx
            .await
            .map_err(|_| AppError::internal("ACP session actor exited during startup"))?;
        result?;
        let mut entry = SessionEntry {
            info,
            state: SessionState::Created,
            commands,
            stderr_tail,
            prompt_cancel,
        };
        // Successful startup publishes as idle; status must match the enum.
        entry.apply_state(SessionState::Idle);
        let published = entry.info.clone();
        self.sessions
            .write()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))?
            .insert(id, entry);
        // The actor waits for this handoff before accepting commands. That
        // eliminates the race where a connection could exit just after startup
        // but before its owning local session entry was registered.
        let _ = registered_tx.send(());
        Ok(published)
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
        attachments: &[Attachment],
    ) -> Result<(), AppError> {
        // Enqueue the actor Prompt without yielding so Cancel cannot slip onto
        // an empty command channel between reservation and enqueue. Lifecycle
        // events are persisted inside `await_prompt` after the actor owns the
        // turn. A sticky `prompt_cancel` bit still covers Cancel-before-dequeue.
        let sender = self.begin_prompt(session_id)?;
        let (result_tx, result_rx) = oneshot::channel();
        if sender
            .try_send(ActorCommand::Prompt {
                content: content.to_string(),
                attachments: attachments.to_vec(),
                result: result_tx,
            })
            .is_err()
        {
            self.update_state(session_id, SessionState::Failed);
            append_payload(
                &self.deps.event_bus,
                session_id,
                EventPayload::AgentExited {
                    content: "ACP session actor is unavailable".to_string(),
                },
            )
            .await
            .map_err(|error| {
                tracing::error!(
                    session_id,
                    error = %error,
                    "failed to persist ACP prompt-dispatch failure"
                );
                error
            })?;
            return Err(AppError::internal("ACP session actor is unavailable"));
        }
        let prompt_result = result_rx
            .await
            .map_err(|_| AppError::internal("ACP prompt actor exited"))?;
        prompt_result?;
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
        let (sender, prompt_cancel) = self.session_for_command(session_id)?;
        // Mark sticky cancel before the actor observes Cancel so a prompt
        // reserved but not yet dequeued still fails when it starts.
        prompt_cancel.store(true, Ordering::Release);
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
        attachments: Vec<Attachment>,
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
    event_bus: SharedEventBus,
    stderr_tail: Arc<Mutex<StderrTail>>,
    sessions: Arc<RwLock<HashMap<String, SessionEntry>>>,
    prompt_cancel: Arc<AtomicBool>,
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
        Ok(ActorExit::Closed(close_result)) => {
            let _ = close_result.send(());
        }
        Err(error) => {
            if let Some(ready) = ready.take() {
                let _ = ready.send(Err(startup_error(&error, &config.stderr_tail)));
            } else if let Err(append_error) = append_payload(
                &config.event_bus,
                &config.local_session_id,
                EventPayload::AgentExited {
                    content: "ACP session actor exited unexpectedly".to_string(),
                },
            )
            .await
            {
                // The original actor error still determines the session state;
                // only the stable error category is logged to avoid exposing
                // agent-provided diagnostics that may contain secrets.
                tracing::error!(
                    session_id = %config.local_session_id,
                    error = %append_error,
                    "failed to persist ACP actor-exit event"
                );
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
            entry.apply_state(SessionState::Failed);
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
                async move |request: ReadTextFileRequest, responder, _cx: ConnectionTo<Agent>| {
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
                async move |request: WriteTextFileRequest, responder, _cx: ConnectionTo<Agent>| {
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
                async move |request: RequestPermissionRequest,
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
                async move |request: CreateTerminalRequest, responder, _cx: ConnectionTo<Agent>| {
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP denied terminal create",
                        move |deps| async move { create_terminal(deps, request) },
                    );
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let deps = handler_deps.clone();
                async move |request: TerminalOutputRequest, responder, _cx: ConnectionTo<Agent>| {
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP terminal output failed",
                        move |deps| async move { terminal_output(deps, request) },
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
                async move |request: WaitForTerminalExitRequest,
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
                async move |request: KillTerminalRequest, responder, _cx: ConnectionTo<Agent>| {
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP terminal kill failed",
                        move |deps| async move { kill_terminal(deps, request) },
                    );
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let deps = handler_deps;
                async move |request: ReleaseTerminalRequest, responder, _cx: ConnectionTo<Agent>| {
                    spawn_result_callback(
                        deps.clone(),
                        responder,
                        "ACP terminal release failed",
                        move |deps| async move { release_terminal(deps, request) },
                    );
                    Ok(())
                }
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
            actor_loop(
                cx,
                session.session_id,
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
    let _ = child.kill();
    let _ = child.status().await;
    connected.map_err(|error| AppError::internal(format!("ACP connection: {error}")))
}

/// Outcome returned by the connection-owning actor loop.
enum ActorExit {
    Closed(oneshot::Sender<()>),
}

async fn actor_loop(
    cx: ConnectionTo<Agent>,
    agent_session_id: SessionId,
    commands: &mut mpsc::Receiver<ActorCommand>,
    event_bus: SharedEventBus,
    local_session_id: String,
    prompt_cancel: Arc<AtomicBool>,
) -> Result<ActorExit, agent_client_protocol::Error> {
    while let Some(command) = commands.recv().await {
        match command {
            ActorCommand::Prompt {
                content,
                attachments,
                result,
            } => {
                match await_prompt(PromptTurn {
                    cx: cx.clone(),
                    agent_session_id: agent_session_id.clone(),
                    content,
                    attachments,
                    result,
                    commands,
                    event_bus: &event_bus,
                    local_session_id: &local_session_id,
                    prompt_cancel: &prompt_cancel,
                })
                .await?
                {
                    PromptExit::Continue => {}
                    PromptExit::Closed(result) => return Ok(ActorExit::Closed(result)),
                }
            }
            ActorCommand::Cancel => {
                send_cancel(&cx, &agent_session_id)?;
            }
            ActorCommand::Close(result) => {
                return Ok(ActorExit::Closed(result));
            }
        }
    }
    Err(agent_client_protocol::Error::internal_error())
}

enum PromptExit {
    Continue,
    Closed(oneshot::Sender<()>),
}

/// Stable wire spelling for ACP stop reasons, including an SDK-forward fallback.
fn stop_reason_name(reason: agent_client_protocol::schema::v1::StopReason) -> &'static str {
    use agent_client_protocol::schema::v1::StopReason;

    match reason {
        StopReason::EndTurn => "end_turn",
        StopReason::MaxTokens => "max_tokens",
        StopReason::MaxTurnRequests => "max_turn_requests",
        StopReason::Refusal => "refusal",
        StopReason::Cancelled => "cancelled",
        _ => "unknown",
    }
}

struct PromptTurn<'a> {
    cx: ConnectionTo<Agent>,
    agent_session_id: SessionId,
    content: String,
    attachments: Vec<Attachment>,
    result: oneshot::Sender<Result<(), AppError>>,
    commands: &'a mut mpsc::Receiver<ActorCommand>,
    event_bus: &'a SharedEventBus,
    local_session_id: &'a str,
    prompt_cancel: &'a AtomicBool,
}

/// Await one prompt while continuing to receive session control commands.
async fn await_prompt(turn: PromptTurn<'_>) -> Result<PromptExit, agent_client_protocol::Error> {
    let PromptTurn {
        cx,
        agent_session_id,
        content,
        attachments,
        result,
        commands,
        event_bus,
        local_session_id,
        prompt_cancel,
    } = turn;

    // Cancel may have won the race onto an idle actor before this Prompt was
    // dequeued. Honor the sticky bit before touching the agent.
    if prompt_cancel.swap(false, Ordering::AcqRel) {
        let cancel = send_cancel(&cx, &agent_session_id);
        let _ = result.send(Err(AppError::internal("ACP prompt cancelled")));
        cancel?;
        return Ok(PromptExit::Continue);
    }

    // Persist lifecycle events only after the actor owns this turn so Cancel
    // cannot race onto an idle loop and become a no-op.
    if let Err(error) = append_payload(
        event_bus,
        local_session_id,
        EventPayload::PromptSubmitted {
            role: "user".to_string(),
            content: content.clone(),
            attachments,
        },
    )
    .await
    {
        tracing::error!(
            session_id = local_session_id,
            error = %error,
            "failed to persist ACP prompt-submitted event"
        );
        let _ = result.send(Err(error));
        return Ok(PromptExit::Continue);
    }
    if let Err(error) = append_payload(
        event_bus,
        local_session_id,
        // The typed contract contains no response-start text field. The
        // wire adapter therefore emits the stable role-only shape.
        EventPayload::ResponseStarted {
            role: "agent".to_string(),
        },
    )
    .await
    {
        tracing::error!(
            session_id = local_session_id,
            error = %error,
            "failed to persist ACP response-started event"
        );
        let _ = result.send(Err(error));
        return Ok(PromptExit::Continue);
    }

    // Drain control commands that arrived while persisting lifecycle events
    // so Cancel/Close cannot sit behind a prompt that has not started yet.
    while let Ok(command) = commands.try_recv() {
        match command {
            ActorCommand::Cancel => {
                let cancel = send_cancel(&cx, &agent_session_id);
                let _ = result.send(Err(AppError::internal("ACP prompt cancelled")));
                cancel?;
                return Ok(PromptExit::Continue);
            }
            ActorCommand::Close(close) => {
                let _ = result.send(Err(AppError::internal("ACP session closed during prompt")));
                return Ok(PromptExit::Closed(close));
            }
            ActorCommand::Prompt { result: nested, .. } => {
                let _ = nested.send(Err(AppError::validation(
                    "ACP session already has an active prompt",
                )));
            }
        }
    }
    if prompt_cancel.swap(false, Ordering::AcqRel) {
        let cancel = send_cancel(&cx, &agent_session_id);
        let _ = result.send(Err(AppError::internal("ACP prompt cancelled")));
        cancel?;
        return Ok(PromptExit::Continue);
    }

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
                    match reply {
                        Ok(response) => {
                            let final_event = EventPayload::StreamUpdate {
                                role: "agent".to_string(),
                                content: String::new(),
                                streaming: false,
                                thought: false,
                                stop_reason: stop_reason_name(response.stop_reason).to_string(),
                            };
                            append_payload(event_bus, local_session_id, final_event)
                                .await
                                .map_err(|error| {
                                    tracing::error!(
                                        session_id = local_session_id,
                                        error = %error,
                                        "failed to persist ACP prompt-complete event"
                                    );
                                    agent_client_protocol::Error::internal_error()
                                })?;
                            let _ = result.send(Ok(()));
                        }
                        Err(error) => {
                            // Do not copy SDK error text into events/logs:
                            // agents control it and it can contain prompt data.
                            tracing::warn!(
                                session_id = local_session_id,
                                "ACP prompt request failed"
                            );
                            append_payload(
                                event_bus,
                                local_session_id,
                                EventPayload::AgentExited {
                                    content: "ACP prompt request failed".to_string(),
                                },
                            )
                            .await
                            .map_err(|append_error| {
                                tracing::error!(
                                    session_id = local_session_id,
                                    error = %append_error,
                                    "failed to persist ACP prompt-failure event"
                                );
                                agent_client_protocol::Error::internal_error()
                            })?;
                            let _ = result.send(Err(AppError::internal(format!(
                                "ACP prompt: {error}"
                            ))));
                        }
                    }
                }
                return Ok(PromptExit::Continue);
            }
            command = commands.recv() => {
                match command {
                    Some(ActorCommand::Cancel) => {
                        let cancel = send_cancel(&cx, &agent_session_id);
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
                        return Ok(PromptExit::Closed(close));
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

/// Notify the agent that the local session cancelled an in-flight turn.
fn send_cancel(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &SessionId,
) -> Result<(), agent_client_protocol::Error> {
    cx.send_notification(CancelNotification::new(agent_session_id.clone()))
        .map_err(|_| agent_client_protocol::Error::internal_error())
}

#[derive(Clone)]
struct HandlerDeps {
    local_session_id: String,
    workspace_id: String,
    workspace_path: PathBuf,
    workspaces: Arc<dyn WorkspaceManager>,
    permissions: Arc<dyn PermissionManager>,
    event_bus: SharedEventBus,
    terminals: TerminalRegistry,
    cancellation: CancellationToken,
    callback_slots: Arc<Semaphore>,
}

/// Translate, persist, and publish an inbound ACP update in receive order.
///
/// The SDK dispatches notifications in stream order. Awaiting the durable
/// append here keeps that order through SQLite before subscribers observe it.
async fn handle_session_notification(
    deps: &HandlerDeps,
    notification: SessionNotification,
) -> Result<(), AppError> {
    let Some(payload) = super::stream::session_update_to_payload(&notification.update) else {
        return Ok(());
    };
    append_payload(&deps.event_bus, &deps.local_session_id, payload).await
}

/// Project a typed event through the only public wire adapter, then persist it
/// before broadcasting to live listeners. An ID of zero requests SQLite's
/// autoincrement assignment and is replaced before publication.
async fn append_payload(
    event_bus: &SharedEventBus,
    session_id: &str,
    payload: EventPayload,
) -> Result<(), AppError> {
    let typed = TypedEvent {
        meta: EventMeta {
            id: 0,
            session_id: session_id.to_string(),
            timestamp: Utc::now(),
        },
        payload,
    };
    event_bus
        .append_and_publish(typed_event_to_wire(&typed))
        .await?;
    Ok(())
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

/// Bound an inbound ACP request that maps `Result` to typed success/error replies.
fn spawn_result_callback<T, F, Fut>(
    deps: HandlerDeps,
    responder: Responder<T>,
    warn: &'static str,
    work: F,
) where
    T: JsonRpcResponse,
    F: FnOnce(HandlerDeps) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, AppError>> + Send + 'static,
{
    let Some(permit) = callback_permit(&deps) else {
        let _ = responder.respond_with_internal_error(callback_limit_error());
        return;
    };
    spawn_callback(deps.cancellation.clone(), permit, async move {
        match work(deps).await {
            Ok(response) => {
                let _ = responder.respond(response);
            }
            Err(error) => {
                tracing::warn!(error = %error, message = warn);
                let _ = responder.respond_with_internal_error(error);
            }
        }
    });
}

/// Bound an inbound ACP request that always replies with a typed success value.
///
/// Used by `RequestPermission`, which maps failures to `Cancelled` outcomes
/// instead of JSON-RPC internal errors.
fn spawn_respond_callback<T, F, Fut>(deps: HandlerDeps, responder: Responder<T>, work: F)
where
    T: JsonRpcResponse,
    F: FnOnce(HandlerDeps) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T> + Send + 'static,
{
    let Some(permit) = callback_permit(&deps) else {
        let _ = responder.respond_with_internal_error(callback_limit_error());
        return;
    };
    spawn_callback(deps.cancellation.clone(), permit, async move {
        let response = work(deps).await;
        let _ = responder.respond(response);
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
    let path = workspace_relative_path(&deps.workspace_path, &request.path)?;
    deps.workspaces
        .read_file(&deps.workspace_id, &path)
        .await
        .map(|result| ReadTextFileResponse::new(result.content))
}

async fn write_text_file(
    deps: HandlerDeps,
    request: WriteTextFileRequest,
) -> Result<WriteTextFileResponse, AppError> {
    let path = workspace_relative_path(&deps.workspace_path, &request.path)?;
    deps.workspaces
        .write_file(&deps.workspace_id, &path, &request.content, 0)
        .await
        .map(|_| WriteTextFileResponse::new())
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

fn workspace_relative_path(root: &Path, path: &Path) -> Result<String, AppError> {
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
    use crate::events::{EventBus, Store};
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
        let event_bus = Arc::new(EventBus::new(
            Store::open(tempdir.path().join("events.db")).expect("open test event store"),
        ));
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
            event_bus,
        }));
        let session = client
            .create_session("mock", "mock-model", &workspace.id)
            .await
            .expect("create mock ACP session");
        assert_eq!(
            session.status, "idle",
            "successful startup must publish idle metadata"
        );
        assert_eq!(
            client
                .get_session_info(&session.id)
                .expect("session remains registered after startup")
                .status,
            "idle"
        );
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
