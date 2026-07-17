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
use tokio::sync::{mpsc, oneshot, watch};
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
    sessions: RwLock<HashMap<String, SessionEntry>>,
}

impl Client {
    /// Creates an ACP client with all service dependencies supplied up front.
    #[must_use]
    pub fn new(deps: ClientDeps) -> Self {
        Self {
            deps,
            sessions: RwLock::new(HashMap::new()),
        }
    }

    fn session_sender(&self, session_id: &str) -> Result<mpsc::Sender<ActorCommand>, AppError> {
        self.sessions
            .read()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))?
            .get(session_id)
            .map(|entry| entry.commands.clone())
            .ok_or_else(|| AppError::not_found("session"))
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
        let stderr_tail = Arc::new(Mutex::new(StderrTail::default()));
        let actor = ActorConfig {
            agent,
            workspace_id: workspace_id.to_string(),
            workspace_path,
            permissions: Arc::clone(&self.deps.permissions),
            workspaces: Arc::clone(&self.deps.workspaces),
            stderr_tail: Arc::clone(&stderr_tail),
        };
        tokio::spawn(run_actor(actor, receiver, ready_tx));

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
        let sender = self.session_sender(session_id)?;
        let (result_tx, result_rx) = oneshot::channel();
        self.update_state(session_id, SessionState::Running);
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
        let sender = self.session_sender(session_id)?;
        let (closed_tx, closed_rx) = oneshot::channel();
        sender
            .send(ActorCommand::Close(closed_tx))
            .await
            .map_err(|_| AppError::internal("ACP session actor is unavailable"))?;
        let _ = closed_rx.await;
        self.deps.permissions.clear_session(session_id);
        self.sessions
            .write()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))?
            .remove(session_id);
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
    agent: AgentInfo,
    workspace_id: String,
    workspace_path: PathBuf,
    permissions: Arc<dyn PermissionManager>,
    workspaces: Arc<dyn WorkspaceManager>,
    stderr_tail: Arc<Mutex<StderrTail>>,
}

async fn run_actor(
    config: ActorConfig,
    mut commands: mpsc::Receiver<ActorCommand>,
    ready: oneshot::Sender<Result<(), AppError>>,
) {
    let result = run_actor_inner(config, &mut commands, ready).await;
    if let Err(error) = result {
        tracing::warn!(error = %error, "ACP session actor ended");
    }
}

async fn run_actor_inner(
    config: ActorConfig,
    commands: &mut mpsc::Receiver<ActorCommand>,
    ready: oneshot::Sender<Result<(), AppError>>,
) -> Result<(), AppError> {
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
    let read_deps = HandlerDeps {
        workspace_id: config.workspace_id.clone(),
        workspace_path: config.workspace_path.clone(),
        workspaces: Arc::clone(&config.workspaces),
        permissions: Arc::clone(&config.permissions),
        terminals: Arc::clone(&terminals),
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
                tokio::spawn(async move {
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
                tokio::spawn(async move {
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
                tokio::spawn(async move {
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
                tokio::spawn(async move {
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
                tokio::spawn(async move {
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
                tokio::spawn(async move {
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
                tokio::spawn(async move {
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
                tokio::spawn(async move {
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
            let _ = ready.send(Ok(()));
            actor_loop(cx, session.session_id, commands).await
        })
        .await;
    cancel_terminals(&terminals);
    let _ = child.kill();
    let _ = child.status().await;
    connected.map_err(|error| AppError::internal(format!("ACP connection: {error}")))
}

async fn actor_loop(
    cx: ConnectionTo<Agent>,
    agent_session_id: SessionId,
    commands: &mut mpsc::Receiver<ActorCommand>,
) -> Result<(), agent_client_protocol::Error> {
    while let Some(command) = commands.recv().await {
        match command {
            ActorCommand::Prompt { content, result } => {
                let reply = cx
                    .send_request(PromptRequest::new(
                        agent_session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new(content))],
                    ))
                    .block_task()
                    .await
                    .map(|_| ())
                    .map_err(|error| AppError::internal(format!("ACP prompt: {error}")));
                let _ = result.send(reply);
            }
            ActorCommand::Cancel => {
                cx.send_notification(CancelNotification::new(agent_session_id.clone()))
                    .map_err(|_| agent_client_protocol::Error::internal_error())?;
            }
            ActorCommand::Close(result) => {
                let _ = result.send(());
                break;
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct HandlerDeps {
    workspace_id: String,
    workspace_path: PathBuf,
    workspaces: Arc<dyn WorkspaceManager>,
    permissions: Arc<dyn PermissionManager>,
    terminals: TerminalRegistry,
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
    let cwd = terminal_cwd(&deps.workspace_path, request.cwd.as_deref())?;
    let limit = request
        .output_byte_limit
        .map_or(MAX_TERMINAL_OUTPUT_BYTES, |limit| {
            usize::try_from(limit)
                .unwrap_or(MAX_TERMINAL_OUTPUT_BYTES)
                .min(MAX_TERMINAL_OUTPUT_BYTES)
        });
    let terminal_id = format!("term-{}", Uuid::new_v4().simple());
    let cancel = CancellationToken::new();
    let (exit, _) = watch::channel(None);
    let state = Arc::new(TerminalState {
        cancel: cancel.clone(),
        output: Mutex::new(RetainedOutput::new(limit)),
        exit,
    });
    deps.terminals
        .lock()
        .map_err(|_| AppError::internal("ACP terminal registry lock poisoned"))?
        .insert(terminal_id.clone(), Arc::clone(&state));

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
                cancel,
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
    if let Ok(terminals) = registry.lock() {
        for terminal in terminals.values() {
            terminal.cancel.cancel();
        }
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
        session_id: request.session_id.to_string(),
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
    use super::RetainedOutput;

    #[test]
    fn retained_terminal_output_truncates_at_utf8_boundary() {
        let mut output = RetainedOutput::new(5);
        output.push_line("éé");
        output.push_line("x");

        assert!(output.truncated);
        assert!(output.text.len() <= 5);
        assert!(std::str::from_utf8(output.text.as_bytes()).is_ok());
    }
}
