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
    ClientCapabilities, ContentBlock, FileSystemCapabilities, InitializeRequest, NewSessionRequest,
    PromptRequest, SessionId, SessionNotification,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Agent, ByteStreams, Client as SdkClient, ConnectionTo};
use async_process::Command;
use async_trait::async_trait;
use chrono::Utc;
use futures_util::io::AsyncReadExt;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::AgentRegistry;
use crate::config::AgentInfo;
use crate::interfaces::{
    ACPClient, AppError, Attachment, PermissionManager, Session, SessionInfo, WorkspaceManager,
};

/// Maximum retained agent stderr diagnostic tail. Agent stderr is untrusted and
/// must never be allowed to grow the daemon's memory without bound.
pub const STDERR_TAIL_BYTES: usize = 8 * 1024;
const ACTOR_COMMAND_CAPACITY: usize = 32;

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
            workspace_path,
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
        self.update_state(session_id, SessionState::Idle);
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
    workspace_path: PathBuf,
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
    let connected = SdkClient
        .builder()
        .name("local-agent")
        .on_receive_notification(
            async |_notification: SessionNotification, _cx: ConnectionTo<Agent>| Ok(()),
            agent_client_protocol::on_receive_notification!(),
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
                        vec![ContentBlock::Text(
                            agent_client_protocol::schema::v1::TextContent::new(content),
                        )],
                    ))
                    .block_task()
                    .await
                    .map(|_| ())
                    .map_err(|error| AppError::internal(format!("ACP prompt: {error}")));
                let _ = result.send(reply);
            }
            ActorCommand::Cancel => {
                // Dropping a sent request would emit protocol cancellation, but
                // a completed actor command has no request to drop. The SDK's
                // typed Cancel notification is added alongside terminal support.
            }
            ActorCommand::Close(result) => {
                let _ = result.send(());
                break;
            }
        }
    }
    Ok(())
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
