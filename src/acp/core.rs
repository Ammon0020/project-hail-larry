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
    CreateTerminalResponse, EmbeddedResource, EmbeddedResourceResource, FileSystemCapabilities,
    Implementation, InitializeRequest, InitializeResponse, KillTerminalRequest,
    KillTerminalResponse, ListSessionsRequest, LoadSessionRequest, McpCapabilities, McpServer,
    NewSessionRequest, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionConfigOption, SessionId, SessionNotification, TerminalExitStatus, TerminalOutputRequest,
    TerminalOutputResponse, TextContent, TextResourceContents, WaitForTerminalExitRequest,
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
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::providers::{
    find_model_config_id, require_providers_supported, rpc_disable_provider, rpc_list_providers,
    rpc_set_model_config, rpc_set_provider, SessionCaps, MODEL_SWITCH_UNSUPPORTED_MSG,
};
use super::{
    context::{PreparedPrompt, PromptPipeline},
    conversation::{export_conversation, ConversationTransfer},
    store::{ConversationStore, StoredSession},
    AgentRegistry,
};
use crate::config::AgentInfo;
use crate::events::SharedEventBus;
use crate::interfaces::{
    wire::typed_event_to_wire, ACPClient, AppError, Attachment, EventMeta, EventPayload,
    PermissionManager, ProviderInfo, Session, SessionInfo, TypedEvent, WorkspaceInfo,
    WorkspaceManager,
};
use crate::procutil::{configure_process_group, ProcessGroupCleanup};
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
/// Safe default for a model-switch rebind transfer (256 KiB).
const MODEL_SWITCH_TRANSFER_BYTES: i64 = 256 * 1024;

/// ACP `clientInfo.name` — must be non-empty; agents (e.g. Mistral Vibe) forward
/// it into provider metadata that rejects blank values. Matches Go transport.
const ACP_CLIENT_NAME: &str = "LocalAgentInterface";
/// ACP `clientInfo.version` — same Go parity constraint as [`ACP_CLIENT_NAME`].
const ACP_CLIENT_VERSION: &str = "1.0";

/// Constructor-only dependencies for ACP core.
pub struct ClientDeps {
    pub registry: Arc<AgentRegistry>,
    pub workspaces: Arc<dyn WorkspaceManager>,
    pub permissions: Arc<dyn PermissionManager>,
    /// Ordered durable event stream for prompt lifecycle and ACP updates.
    pub event_bus: SharedEventBus,
    /// Optional durable metadata file; `None` is useful for isolated tests.
    pub conversation_store: ConversationStore,
    /// Path to `mcp.json`. `None` skips MCP attachment on session/new (tests).
    pub mcp_config_path: Option<PathBuf>,
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
    /// Capabilities captured from Initialize (providers + embeddedContext).
    caps: SessionCaps,
    /// Config option id for the model selector when the agent advertises one.
    /// Empty/`None` means live `switch_model` is unsupported (no rebind here).
    model_config_id: Option<String>,
    /// Agent-side ACP session id for durable `session/load` resume.
    ///
    /// Persisted in `conversations.json` as `acpSessionId`; never exposed on
    /// REST [`SessionInfo`].
    acp_session_id: String,
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
    /// Durable sessions from `conversations.json` with no live actor yet.
    ///
    /// Loaded at daemon start; actors are spawned lazily on prompt / cancel /
    /// provider / rebind (not at boot). Includes durable `acpSessionId`.
    dormant: Arc<RwLock<HashMap<String, StoredSession>>>,
    /// Serializes dormant→live restores so concurrent ops cannot double-spawn.
    restore_lock: AsyncMutex<()>,
    pipeline: Arc<PromptPipeline>,
}

impl Client {
    /// Creates an ACP client with all service dependencies supplied up front.
    ///
    /// Does not load `conversations.json` — call [`Self::load_conversations`]
    /// after construction so a corrupt store fails daemon startup loudly.
    #[must_use]
    pub fn new(deps: ClientDeps) -> Self {
        Self {
            deps,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            dormant: Arc::new(RwLock::new(HashMap::new())),
            restore_lock: AsyncMutex::new(()),
            pipeline: Arc::new(PromptPipeline::default()),
        }
    }

    /// Returns the frontend context tracker used by prompt middleware.
    #[must_use]
    pub fn open_files_tracker(&self) -> &super::context::OpenFilesTracker {
        &self.pipeline.tracker
    }

    /// Shared poison mapping for the live session registry read lock.
    fn sessions_read(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, HashMap<String, SessionEntry>>, AppError> {
        self.sessions
            .read()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))
    }

    /// Shared poison mapping for the live session registry write lock.
    fn sessions_write(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<String, SessionEntry>>, AppError> {
        self.sessions
            .write()
            .map_err(|_| AppError::internal("ACP session registry lock poisoned"))
    }

    /// Shared poison mapping for the dormant session map read lock.
    fn dormant_read(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, HashMap<String, StoredSession>>, AppError> {
        self.dormant
            .read()
            .map_err(|_| AppError::internal("ACP dormant session lock poisoned"))
    }

    /// Shared poison mapping for the dormant session map write lock.
    fn dormant_write(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<String, StoredSession>>, AppError> {
        self.dormant
            .write()
            .map_err(|_| AppError::internal("ACP dormant session lock poisoned"))
    }

    /// Resolve a registered workspace by id (list + find).
    async fn resolve_workspace(&self, workspace_id: &str) -> Result<WorkspaceInfo, AppError> {
        self.deps
            .workspaces
            .list()
            .await?
            .into_iter()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(|| AppError::not_found_id("workspace", workspace_id))
    }

    /// Look up a live session entry and map it under the registry read lock.
    fn map_live_session<T>(
        &self,
        session_id: &str,
        map: impl FnOnce(&SessionEntry) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let sessions = self.sessions_read()?;
        let entry = sessions
            .get(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
        map(entry)
    }

    /// Load persisted conversation metadata without treating it as live transport state.
    ///
    /// Prefer [`Self::load_conversations`] at daemon start so list/get see
    /// durable sessions. This raw store read remains for diagnostics and
    /// returns the public [`SessionInfo`] projection only.
    pub fn load_conversation_metadata(&self) -> Result<Vec<SessionInfo>, AppError> {
        Ok(self
            .deps
            .conversation_store
            .load()?
            .into_iter()
            .map(|stored| stored.to_info())
            .collect())
    }

    /// Restore durable conversation metadata into the dormant map (no actors).
    ///
    /// Mirrors Go `LoadConversations`: status becomes `idle`, live transports
    /// are not started. `acpSessionId` is preserved for later `session/load`.
    /// Corrupt or unreadable stores return an error so the daemon fails loudly
    /// rather than silently dropping the session list.
    ///
    /// # Errors
    ///
    /// Propagates store I/O/parse failures and poisoned registry locks.
    pub fn load_conversations(&self) -> Result<(), AppError> {
        let records = self.deps.conversation_store.load()?;
        let live = self.sessions_read()?;
        let mut dormant = self.dormant_write()?;
        dormant.clear();
        for mut stored in records {
            if live.contains_key(&stored.info.id) {
                continue;
            }
            // Persisted running/failed bits are stale after restart; idle until
            // the next prompt lazily starts an actor.
            stored.info.status = SessionState::Idle.as_str().to_string();
            dormant.insert(stored.info.id.clone(), stored);
        }
        tracing::info!(
            count = dormant.len(),
            "loaded persisted ACP conversations; actors deferred until use"
        );
        Ok(())
    }

    fn persist_sessions(&self) -> Result<(), AppError> {
        let live = self.sessions_read()?;
        let dormant = self.dormant_read()?;
        // Live metadata wins for shared ids; dormant fills the rest so a
        // create/rename cannot wipe conversations that have not been restored.
        let mut by_id: HashMap<String, StoredSession> =
            HashMap::with_capacity(live.len() + dormant.len());
        for stored in dormant.values() {
            by_id.insert(stored.info.id.clone(), stored.clone());
        }
        for entry in live.values() {
            by_id.insert(
                entry.info.id.clone(),
                StoredSession::from_parts(entry.info.clone(), entry.acp_session_id.clone()),
            );
        }
        let sessions = by_id.into_values().collect::<Vec<_>>();
        self.deps.conversation_store.persist(&sessions)
    }

    fn has_live_session(&self, session_id: &str) -> Result<bool, AppError> {
        Ok(self.sessions_read()?.contains_key(session_id))
    }

    /// Spawn an actor for a stored-but-not-live session, reusing durable
    /// metadata. EventBus history keyed by `session_id` is left intact.
    ///
    /// # Errors
    ///
    /// Returns not-found when the id is neither live nor dormant, or any
    /// agent/workspace/startup failure from the restore handshake.
    async fn ensure_live_session(&self, session_id: &str) -> Result<(), AppError> {
        if self.has_live_session(session_id)? {
            return Ok(());
        }
        let _guard = self.restore_lock.lock().await;
        if self.has_live_session(session_id)? {
            return Ok(());
        }
        let stored = {
            let dormant = self.dormant_read()?;
            dormant
                .get(session_id)
                .cloned()
                .ok_or_else(|| AppError::not_found_id("session", session_id))?
        };
        tracing::info!(
            session_id,
            agent_id = %stored.info.agent_id,
            model_id = %stored.info.model_id,
            has_acp_session_id = !stored.acp_session_id.is_empty(),
            "lazily restoring ACP session actor from durable metadata"
        );
        // Keep the dormant entry until registration succeeds so list/get never
        // briefly lose the conversation if handshake fails.
        self.register_live_session(stored.info.clone(), stored.acp_session_id)
            .await?;
        self.dormant_write()?.remove(session_id);
        self.persist_sessions()?;
        tracing::info!(session_id, "ACP session actor restored");
        Ok(())
    }

    /// Start an agent actor and publish it under `info.id` without wiping history.
    ///
    /// Shared by [`ACPClient::create_session`] and lazy restore. The caller owns
    /// durable persistence and dormant-map bookkeeping.
    ///
    /// `persisted_acp_session_id` is the durable agent-side id for
    /// `session/load` (empty on create; cleared on rebind).
    async fn register_live_session(
        &self,
        info: SessionInfo,
        persisted_acp_session_id: String,
    ) -> Result<SessionInfo, AppError> {
        let agent = self
            .deps
            .registry
            .resolve(&info.agent_id, &info.model_id)
            .map_err(AppError::validation)?;
        let workspace = self.resolve_workspace(&info.workspace).await?;
        let workspace_path = PathBuf::from(workspace.path);
        let id = info.id.clone();
        let (commands, receiver) = mpsc::channel(ACTOR_COMMAND_CAPACITY);
        let (ready_tx, ready_rx) = oneshot::channel();
        let (registered_tx, registered_rx) = oneshot::channel();
        let stderr_tail = Arc::new(Mutex::new(StderrTail::default()));
        let prompt_cancel = Arc::new(AtomicBool::new(false));
        let actor = ActorConfig {
            local_session_id: id.clone(),
            agent,
            workspace_id: info.workspace.clone(),
            workspace_path,
            permissions: Arc::clone(&self.deps.permissions),
            workspaces: Arc::clone(&self.deps.workspaces),
            stderr_tail: Arc::clone(&stderr_tail),
            sessions: Arc::clone(&self.sessions),
            event_bus: Arc::clone(&self.deps.event_bus),
            prompt_cancel: Arc::clone(&prompt_cancel),
            mcp_config_path: self.deps.mcp_config_path.clone(),
            persisted_acp_session_id,
        };
        tokio::spawn(run_actor(actor, receiver, ready_tx, registered_rx));

        let result = ready_rx
            .await
            .map_err(|_| AppError::internal("ACP session actor exited during startup"))?;
        let startup = result?;
        let mut entry = SessionEntry {
            info,
            state: SessionState::Created,
            commands,
            stderr_tail,
            prompt_cancel,
            caps: startup.caps,
            model_config_id: startup.model_config_id,
            acp_session_id: startup.acp_session_id,
        };
        entry.apply_state(SessionState::Idle);
        let published = entry.info.clone();
        self.sessions_write()?.insert(id, entry);
        let _ = registered_tx.send(());
        Ok(published)
    }

    fn session_for_command(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<ActorCommand>, Arc<AtomicBool>), AppError> {
        self.map_live_session(session_id, |entry| match entry.state {
            SessionState::Failed => Err(AppError::internal(
                "ACP session failed; close it and create a new session",
            )),
            SessionState::Closed => Err(AppError::internal("ACP session is closed")),
            _ => Ok((entry.commands.clone(), Arc::clone(&entry.prompt_cancel))),
        })
    }

    /// Look up a session's command sender and cached initialize capabilities.
    fn session_for_providers(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<ActorCommand>, SessionCaps), AppError> {
        self.map_live_session(session_id, |entry| Ok((entry.commands.clone(), entry.caps)))
    }

    /// Look up command sender + model config id for a live model switch.
    fn session_for_model_switch(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<ActorCommand>, Option<String>), AppError> {
        self.map_live_session(session_id, |entry| {
            Ok((entry.commands.clone(), entry.model_config_id.clone()))
        })
    }

    /// Reserve the session's sole prompt slot before enqueuing the actor command.
    fn begin_prompt(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<ActorCommand>, SessionCaps, String), AppError> {
        let mut sessions = self.sessions_write()?;
        let entry = sessions
            .get_mut(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
        match entry.state {
            SessionState::Idle | SessionState::Interrupted => {
                entry.prompt_cancel.store(false, Ordering::Release);
                entry.apply_state(SessionState::Running);
                Ok((
                    entry.commands.clone(),
                    entry.caps,
                    entry.info.workspace.clone(),
                ))
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
        if let Ok(mut sessions) = self.sessions_write() {
            if let Some(entry) = sessions.get_mut(session_id) {
                entry.apply_state(state);
            }
        }
    }

    /// Move a session only when no concurrent lifecycle operation superseded it.
    fn update_state_if(&self, session_id: &str, expected: SessionState, state: SessionState) {
        if let Ok(mut sessions) = self.sessions_write() {
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
            .sessions_read()?
            .get(session_id)
            .map(|entry| Arc::clone(&entry.stderr_tail))
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
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
        // Validate early so a bad agent/workspace fails before allocating an id.
        let _agent = self
            .deps
            .registry
            .resolve(agent_id, model_id)
            .map_err(AppError::validation)?;
        let _workspace = self.resolve_workspace(workspace_id).await?;
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
        let published = self.register_live_session(info, String::new()).await?;
        if let Err(error) = self.persist_sessions() {
            // A failed create must not leave a live, invisible session behind.
            // Teardown is best-effort here; callers receive the original durable
            // persistence failure, which is the actionable error.
            tracing::error!(session_id = %published.id, error = %error, "failed to persist new ACP session");
            let _ = self.close_session(&published.id).await;
            return Err(error);
        }
        Ok(published)
    }

    fn get_session_info(&self, session_id: &str) -> Result<SessionInfo, AppError> {
        if let Some(info) = self
            .sessions_read()?
            .get(session_id)
            .map(|entry| entry.info.clone())
        {
            return Ok(info);
        }
        self.dormant_read()?
            .get(session_id)
            .map(StoredSession::to_info)
            .ok_or_else(|| AppError::not_found_id("session", session_id))
    }

    /// Live-only projection of negotiated session-history caps (S-HIST-PROBE).
    ///
    /// Does **not** cold-start an agent (epic Q8 still open). Dormant sessions
    /// return [`SessionHistoryCapabilities::unavailable`]; unknown ids 404.
    fn session_history_capabilities(
        &self,
        session_id: &str,
    ) -> Result<crate::interfaces::SessionHistoryCapabilities, AppError> {
        if let Some(caps) = self
            .sessions_read()?
            .get(session_id)
            .map(|entry| entry.caps)
        {
            return Ok(caps.to_history_capabilities(true));
        }
        if self.dormant_read()?.contains_key(session_id) {
            tracing::debug!(
                session_id,
                "session history caps unavailable: agent not live (cold-start deferred to Q8)"
            );
            return Ok(crate::interfaces::SessionHistoryCapabilities::unavailable());
        }
        Err(AppError::not_found_id("session", session_id))
    }

    fn list_sessions(&self) -> Vec<Session> {
        let Ok(live) = self.sessions_read() else {
            tracing::error!("ACP session registry lock poisoned during list_sessions");
            return Vec::new();
        };
        let Ok(dormant) = self.dormant_read() else {
            tracing::error!("ACP dormant session lock poisoned during list_sessions");
            return Vec::new();
        };
        let mut by_id: HashMap<String, SessionInfo> =
            HashMap::with_capacity(live.len() + dormant.len());
        for stored in dormant.values() {
            by_id.insert(stored.info.id.clone(), stored.to_info());
        }
        // Live status/timestamps win when both maps contain the same id.
        for entry in live.values() {
            by_id.insert(entry.info.id.clone(), entry.info.clone());
        }
        let mut values: Vec<_> = by_id.into_values().collect();
        values.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        values
    }

    async fn send_prompt(
        &self,
        session_id: &str,
        content: &str,
        attachments: &[Attachment],
    ) -> Result<(), AppError> {
        // Lazily restore a durable session after daemon restart before reserving
        // the prompt slot. Event history stays in the EventBus under this id.
        self.ensure_live_session(session_id).await?;
        // Enqueue the actor Prompt without yielding so Cancel cannot slip onto
        // an empty command channel between reservation and enqueue. Lifecycle
        // events are persisted inside `await_prompt` after the actor owns the
        // turn. A sticky `prompt_cancel` bit still covers Cancel-before-dequeue.
        let (sender, caps, workspace_id) = self.begin_prompt(session_id)?;
        let workspace = self.resolve_workspace(&workspace_id).await?;
        let prepared = match self
            .pipeline
            .prepare(
                session_id,
                &workspace_id,
                Path::new(&workspace.path),
                caps.embedded_context,
                self.deps.workspaces.as_ref(),
            )
            .await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                self.update_state_if(session_id, SessionState::Running, SessionState::Idle);
                return Err(error);
            }
        };
        let (result_tx, result_rx) = oneshot::channel();
        if sender
            .try_send(ActorCommand::Prompt {
                user_content: content.to_string(),
                prepared,
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
        {
            let mut sessions = self.sessions_write()?;
            if let Some(entry) = sessions.get_mut(session_id) {
                entry.info.name = name.to_string();
                entry.info.updated_at = Utc::now();
                drop(sessions);
                self.persist_sessions()?;
                return Ok(());
            }
        }
        {
            let mut dormant = self.dormant_write()?;
            let entry = dormant
                .get_mut(session_id)
                .ok_or_else(|| AppError::not_found_id("session", session_id))?;
            entry.info.name = name.to_string();
            entry.info.updated_at = Utc::now();
        }
        self.persist_sessions()?;
        Ok(())
    }

    async fn rebind_session(
        &self,
        session_id: &str,
        agent_id: &str,
        model_id: &str,
        max_transfer_bytes: i64,
    ) -> Result<SessionInfo, AppError> {
        self.ensure_live_session(session_id).await?;
        let agent = self
            .deps
            .registry
            .resolve(agent_id, model_id)
            .map_err(AppError::validation)?;
        let (workspace_id, old_agent_id, commands) = {
            let mut sessions = self.sessions_write()?;
            let entry = sessions
                .get_mut(session_id)
                .ok_or_else(|| AppError::not_found_id("session", session_id))?;
            if entry.state != SessionState::Idle {
                return Err(AppError::validation(
                    "ACP session must be idle before it can be rebound",
                ));
            }
            // Created gates new prompts while the old actor is drained and the
            // replacement handshake runs; history stays entirely in EventBus.
            entry.apply_state(SessionState::Created);
            (
                entry.info.workspace.clone(),
                entry.info.agent_id.clone(),
                entry.commands.clone(),
            )
        };
        let transfer =
            match export_conversation(&self.deps.event_bus, session_id, max_transfer_bytes).await {
                Ok(markdown) => markdown,
                Err(error) => {
                    self.update_state_if(session_id, SessionState::Created, SessionState::Idle);
                    return Err(error);
                }
            };
        let workspace = match self.resolve_workspace(&workspace_id).await {
            Ok(workspace) => workspace,
            Err(error) => {
                self.update_state_if(session_id, SessionState::Created, SessionState::Idle);
                return Err(error);
            }
        };

        let (closed_tx, closed_rx) = oneshot::channel();
        commands
            .send(ActorCommand::Close(closed_tx))
            .await
            .map_err(|_| AppError::internal("ACP session actor is unavailable during rebind"))?;
        let _ = closed_rx.await;

        let stderr_tail = Arc::new(Mutex::new(StderrTail::default()));
        let prompt_cancel = Arc::new(AtomicBool::new(false));
        let (new_commands, receiver) = mpsc::channel(ACTOR_COMMAND_CAPACITY);
        let (ready_tx, ready_rx) = oneshot::channel();
        let (registered_tx, registered_rx) = oneshot::channel();
        tokio::spawn(run_actor(
            ActorConfig {
                local_session_id: session_id.to_string(),
                agent,
                workspace_id: workspace_id.clone(),
                workspace_path: PathBuf::from(workspace.path),
                permissions: Arc::clone(&self.deps.permissions),
                workspaces: Arc::clone(&self.deps.workspaces),
                stderr_tail: Arc::clone(&stderr_tail),
                sessions: Arc::clone(&self.sessions),
                event_bus: Arc::clone(&self.deps.event_bus),
                prompt_cancel: Arc::clone(&prompt_cancel),
                mcp_config_path: self.deps.mcp_config_path.clone(),
                // Rebind switches agents; clear the prior ACP id so we never
                // attempt session/load against the wrong agent (Go parity).
                persisted_acp_session_id: String::new(),
            },
            receiver,
            ready_tx,
            registered_rx,
        ));
        let startup = match ready_rx.await {
            Ok(Ok(startup)) => startup,
            Ok(Err(error)) => {
                self.update_state(session_id, SessionState::Failed);
                return Err(error);
            }
            Err(_) => {
                self.update_state(session_id, SessionState::Failed);
                return Err(AppError::internal(
                    "ACP replacement actor exited during startup",
                ));
            }
        };
        let updated = {
            let mut sessions = self.sessions_write()?;
            let entry = sessions
                .get_mut(session_id)
                .ok_or_else(|| AppError::not_found_id("session", session_id))?;
            entry.commands = new_commands;
            entry.stderr_tail = stderr_tail;
            entry.prompt_cancel = prompt_cancel;
            entry.caps = startup.caps;
            entry.model_config_id = startup.model_config_id;
            entry.acp_session_id = startup.acp_session_id;
            entry.info.agent_id = agent_id.to_string();
            entry.info.model_id = model_id.to_string();
            entry.apply_state(SessionState::Idle);
            entry.info.clone()
        };
        // The replacement actor has initialized, and the entry now owns its
        // sender, so it may safely start receiving commands.
        let _ = registered_tx.send(());
        self.pipeline.reset(session_id);
        self.pipeline.queue_transfer(
            session_id.to_string(),
            ConversationTransfer {
                markdown: transfer,
                from_agent_name: old_agent_id,
            },
        )?;
        append_payload(
            &self.deps.event_bus,
            session_id,
            EventPayload::ConnectionRestarted {
                content: format!("Rebound session to {agent_id}/{model_id}."),
            },
        )
        .await?;
        self.persist_sessions()?;
        tracing::info!(
            session_id,
            agent_id,
            model_id,
            "ACP session rebound without history wipe"
        );
        Ok(updated)
    }

    async fn switch_model(&self, session_id: &str, model_id: &str) -> Result<(), AppError> {
        self.ensure_live_session(session_id).await?;
        let (sender, model_config_id) = self.session_for_model_switch(session_id)?;
        let Some(config_id) = model_config_id else {
            let current = self.get_session_info(session_id)?;
            if current.status != SessionState::Idle.as_str() {
                tracing::info!(
                    session_id,
                    model_id,
                    status = current.status,
                    "switch_model left live provider state unchanged: rebind is not clean"
                );
                return Err(AppError::unsupported(format!(
                    "{MODEL_SWITCH_UNSUPPORTED_MSG}; session must be idle for rebind fallback"
                )));
            }
            tracing::info!(
                session_id,
                model_id,
                "switch_model falling back to clean rebind"
            );
            return self
                .rebind_session(
                    session_id,
                    &current.agent_id,
                    model_id,
                    MODEL_SWITCH_TRANSFER_BYTES,
                )
                .await
                .map(|_| ());
        };
        let (result_tx, result_rx) = oneshot::channel();
        sender
            .send(ActorCommand::SwitchModel {
                config_id,
                model_id: model_id.to_string(),
                result: result_tx,
            })
            .await
            .map_err(|_| AppError::internal("ACP session actor is unavailable"))?;
        result_rx
            .await
            .map_err(|_| AppError::internal("ACP switch_model actor exited"))??;

        {
            let mut sessions = self.sessions_write()?;
            let entry = sessions
                .get_mut(session_id)
                .ok_or_else(|| AppError::not_found_id("session", session_id))?;
            entry.info.model_id = model_id.to_string();
            entry.info.updated_at = Utc::now();
        }
        self.persist_sessions()?;

        append_payload(
            &self.deps.event_bus,
            session_id,
            EventPayload::ModelChanged {
                content: format!("Switched model to {model_id}."),
            },
        )
        .await?;
        Ok(())
    }

    async fn cancel_session(&self, session_id: &str) -> Result<(), AppError> {
        // Stored-only sessions still need an actor so Cancel can reach the agent
        // once a concurrent prompt restore races in; mirrors Go lazy start.
        self.ensure_live_session(session_id).await?;
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
        // Dormant-only: delete durable metadata without spawning an agent just
        // to tear it down (spawn failure would otherwise block conversation delete).
        if !self.has_live_session(session_id)? {
            let removed = self.dormant_write()?.remove(session_id);
            if removed.is_none() {
                return Err(AppError::not_found_id("session", session_id));
            }
            self.deps.permissions.clear_session(session_id);
            self.pipeline.clear(session_id);
            tracing::info!(
                session_id,
                "closed dormant ACP session without actor restore"
            );
            return self.persist_sessions();
        }
        // Removing first makes close idempotent from the public registry's
        // perspective and prevents new work from being queued while teardown
        // is in progress. The actor still owns the sender copied below.
        let entry = self
            .sessions_write()?
            .remove(session_id)
            .ok_or_else(|| AppError::not_found_id("session", session_id))?;
        // Closing removes only live transport state; durable events remain in
        // SQLite and the metadata list is atomically updated before return.
        // Drop any stale dormant twin so persist cannot resurrect this id.
        let _ = self.dormant_write()?.remove(session_id);
        let persist_result = self.persist_sessions();
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
        self.pipeline.clear(session_id);
        persist_result
    }

    fn set_session_profile(&self, session_id: &str, profile: &str) {
        if let Err(error) = self.pipeline.profiles.set_profile(session_id, profile) {
            tracing::error!(session_id, error = %error, "failed to set ACP session profile");
        }
    }

    async fn list_providers(&self, session_id: &str) -> Result<Vec<ProviderInfo>, AppError> {
        self.ensure_live_session(session_id).await?;
        let (sender, caps) = self.session_for_providers(session_id)?;
        require_providers_supported(caps)?;
        let (result_tx, result_rx) = oneshot::channel();
        sender
            .send(ActorCommand::ListProviders { result: result_tx })
            .await
            .map_err(|_| AppError::internal("ACP session actor is unavailable"))?;
        result_rx
            .await
            .map_err(|_| AppError::internal("ACP list_providers actor exited"))?
    }

    async fn set_provider(
        &self,
        session_id: &str,
        id: &str,
        api_type: &str,
        base_url: &str,
        headers: std::collections::HashMap<String, String>,
    ) -> Result<(), AppError> {
        self.ensure_live_session(session_id).await?;
        let (sender, caps) = self.session_for_providers(session_id)?;
        require_providers_supported(caps)?;
        let (result_tx, result_rx) = oneshot::channel();
        sender
            .send(ActorCommand::SetProvider {
                id: id.to_string(),
                api_type: api_type.to_string(),
                base_url: base_url.to_string(),
                headers,
                result: result_tx,
            })
            .await
            .map_err(|_| AppError::internal("ACP session actor is unavailable"))?;
        result_rx
            .await
            .map_err(|_| AppError::internal("ACP set_provider actor exited"))?
    }

    async fn disable_provider(&self, session_id: &str, id: &str) -> Result<(), AppError> {
        self.ensure_live_session(session_id).await?;
        let (sender, caps) = self.session_for_providers(session_id)?;
        require_providers_supported(caps)?;
        let (result_tx, result_rx) = oneshot::channel();
        sender
            .send(ActorCommand::DisableProvider {
                id: id.to_string(),
                result: result_tx,
            })
            .await
            .map_err(|_| AppError::internal("ACP session actor is unavailable"))?;
        result_rx
            .await
            .map_err(|_| AppError::internal("ACP disable_provider actor exited"))?
    }
}

enum ActorCommand {
    Prompt {
        /// User text is persisted verbatim; middleware context is transport-only.
        user_content: String,
        prepared: PreparedPrompt,
        attachments: Vec<Attachment>,
        result: oneshot::Sender<Result<(), AppError>>,
    },
    ListProviders {
        result: oneshot::Sender<Result<Vec<ProviderInfo>, AppError>>,
    },
    SetProvider {
        id: String,
        api_type: String,
        base_url: String,
        headers: std::collections::HashMap<String, String>,
        result: oneshot::Sender<Result<(), AppError>>,
    },
    DisableProvider {
        id: String,
        result: oneshot::Sender<Result<(), AppError>>,
    },
    SwitchModel {
        config_id: String,
        model_id: String,
        result: oneshot::Sender<Result<(), AppError>>,
    },
    Cancel,
    Close(oneshot::Sender<()>),
}

/// Startup handshake result returned before the session is published.
struct ActorStartup {
    caps: SessionCaps,
    model_config_id: Option<String>,
    /// Resolved agent-side ACP session id (from load or new).
    acp_session_id: String,
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
    /// Optional `mcp.json` path passed through to session/new and session/load.
    mcp_config_path: Option<PathBuf>,
    /// Durable agent ACP session id to attempt `session/load` with (empty =
    /// always `session/new`). Cleared on rebind.
    persisted_acp_session_id: String,
}

async fn run_actor(
    config: ActorConfig,
    mut commands: mpsc::Receiver<ActorCommand>,
    ready: oneshot::Sender<Result<ActorStartup, AppError>>,
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
            let mcp_servers = load_session_mcp_servers(
                config.mcp_config_path.as_deref(),
                &init.agent_capabilities.mcp_capabilities,
            );
            let (agent_session_id, config_options) = resolve_acp_session(
                &cx,
                &init,
                &config.workspace_path,
                mcp_servers,
                &config.persisted_acp_session_id,
            )
            .await
            .map_err(|error| {
                if error.to_string().to_ascii_lowercase().contains("authentication") {
                    tracing::error!(
                        "AGENT AUTHENTICATION REQUIRED: The agent CLI rejected the session request. \
                        Please run `{} login` on the host machine running this daemon \
                        to authenticate your environment.",
                        config.agent.command
                    );
                }
                error
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
            let acp_session_id = agent_session_id.to_string();
            if let Some(ready) = ready.take() {
                let _ = ready.send(Ok(ActorStartup {
                    caps,
                    model_config_id,
                    acp_session_id,
                }));
            }
            if let Some(registered) = registered.take() {
                registered
                    .await
                    .map_err(|_| agent_client_protocol::Error::internal_error())?;
            }
            actor_loop(
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
/// 1. If persisted id + load + list: ListSessions by cwd; missing → NewSession;
///    list error → fall through.
/// 2. If persisted id + load: LoadSession; success returns the persisted id.
/// 3. Else / on load failure: NewSession.
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
                user_content,
                prepared,
                attachments,
                result,
            } => {
                match await_prompt(PromptTurn {
                    cx: cx.clone(),
                    agent_session_id: agent_session_id.clone(),
                    user_content,
                    prepared,
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
            other => {
                if let Some(closed) = handle_non_prompt_command(&cx, &agent_session_id, other).await
                {
                    return Ok(ActorExit::Closed(closed));
                }
            }
        }
    }
    Err(agent_client_protocol::Error::internal_error())
}

/// Handle provider / model / cancel / close commands outside a prompt turn.
///
/// Returns `Some(close_ack)` when the session should tear down.
async fn handle_non_prompt_command(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &SessionId,
    command: ActorCommand,
) -> Option<oneshot::Sender<()>> {
    match command {
        ActorCommand::ListProviders { result } => {
            let _ = result.send(rpc_list_providers(cx).await);
            None
        }
        ActorCommand::SetProvider {
            id,
            api_type,
            base_url,
            headers,
            result,
        } => {
            let _ = result.send(rpc_set_provider(cx, id, api_type, base_url, headers).await);
            None
        }
        ActorCommand::DisableProvider { id, result } => {
            let _ = result.send(rpc_disable_provider(cx, id).await);
            None
        }
        ActorCommand::SwitchModel {
            config_id,
            model_id,
            result,
        } => {
            let _ = result
                .send(rpc_set_model_config(cx, agent_session_id, &config_id, &model_id).await);
            None
        }
        ActorCommand::Cancel => {
            if let Err(error) = send_cancel(cx, agent_session_id) {
                tracing::error!(error = %error, "ACP cancel notification failed");
            }
            None
        }
        ActorCommand::Close(result) => Some(result),
        ActorCommand::Prompt { result, .. } => {
            // Nested prompts are rejected at begin_prompt; this is a defensive path.
            let _ = result.send(Err(AppError::validation(
                "ACP session already has an active prompt",
            )));
            None
        }
    }
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
    user_content: String,
    prepared: PreparedPrompt,
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
        user_content,
        prepared,
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
            content: user_content.clone(),
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
    // Provider/model RPCs are serviced here too (concurrent with the upcoming
    // prompt) so they are not starved behind a long turn.
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
            other => {
                if let Some(closed) = handle_non_prompt_command(&cx, &agent_session_id, other).await
                {
                    let _ =
                        result.send(Err(AppError::internal("ACP session closed during prompt")));
                    return Ok(PromptExit::Closed(closed));
                }
            }
        }
    }
    if prompt_cancel.swap(false, Ordering::AcqRel) {
        let cancel = send_cancel(&cx, &agent_session_id);
        let _ = result.send(Err(AppError::internal("ACP prompt cancelled")));
        cancel?;
        return Ok(PromptExit::Continue);
    }

    let mut blocks = vec![ContentBlock::Text(TextContent::new(
        prepared.with_user_text(&user_content),
    ))];
    blocks.extend(prepared.resources.into_iter().map(|resource| {
        ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::TextResourceContents(
                TextResourceContents::new(resource.text, resource.uri)
                    .mime_type(resource.mime_type),
            ),
        ))
    }));
    let prompt = cx
        .send_request(PromptRequest::new(agent_session_id.clone(), blocks))
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
                    Some(other) => {
                        if let Some(closed) =
                            handle_non_prompt_command(&cx, &agent_session_id, other).await
                        {
                            if let Some(result) = result.take() {
                                let _ = result.send(Err(AppError::internal(
                                    "ACP session closed during prompt",
                                )));
                            }
                            return Ok(PromptExit::Closed(closed));
                        }
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
        .ok_or_else(|| AppError::not_found_id("terminal", &request.terminal_id.to_string()))?;
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
        .ok_or_else(|| AppError::not_found_id("terminal", terminal_id))
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

/// Load enabled MCP servers for session/new, filtered by agent capabilities.
///
/// Missing path / missing file / parse errors yield an empty list (Go parity:
/// MCP is additive and must not block session creation).
fn load_session_mcp_servers(path: Option<&Path>, caps: &McpCapabilities) -> Vec<McpServer> {
    let Some(path) = path else {
        return Vec::new();
    };
    match crate::mcp::File::load(path).and_then(|file| file.to_acp(caps)) {
        Ok(servers) => {
            if !servers.is_empty() {
                tracing::debug!(
                    path = %path.display(),
                    count = servers.len(),
                    "attaching MCP servers to session/new"
                );
            }
            servers
        }
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "loading mcp config; continuing without mcp servers"
            );
            Vec::new()
        }
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
        .await?;
    // Broadcast FileWritten so the UI refreshes the explorer. App writes are
    // suppressed in fswatch (note_app_write) to avoid a duplicate
    // FileChangedOnDisk for the same change — without this event the tree
    // would stay stale for agent-created files.
    if let Err(error) = append_payload(
        &deps.event_bus,
        &deps.local_session_id,
        EventPayload::FileWritten {
            workspace_id: deps.workspace_id.clone(),
            target: path,
        },
    )
    .await
    {
        // File is already on disk; failing the ACP response would mislead the
        // agent. Log loudly so a broken event bus is still visible.
        tracing::error!(
            session_id = %deps.local_session_id,
            workspace_id = %deps.workspace_id,
            %error,
            "failed to publish FileWritten after agent write"
        );
    }
    Ok(WriteTextFileResponse::new())
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

    use super::{
        session_exists, should_attempt_load, AgentRegistry, Client, ClientDeps, ConversationStore,
        RetainedOutput, StoredSession,
    };
    use crate::config::{AgentInfo, AgentModel};
    use crate::events::{EventBus, Store};
    use crate::interfaces::{
        ACPClient, AppError, Event, EventStore, EventType, PermissionDecision, PermissionManager,
        PermissionRequest, SessionInfo, WorkspaceManager,
    };
    use crate::workspace::Manager as WorkspaceRegistry;
    use chrono::Utc;

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

    /// Create an isolated ACP client with a configurable conversation store and
    /// no live sessions (used to simulate post-restart dormant metadata).
    async fn mock_client_empty(
        conversation_store: ConversationStore,
    ) -> (Arc<Client>, Arc<RecordingPermissions>, TempDir, String) {
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
            conversation_store,
            mcp_config_path: None,
        }));
        (client, permissions, tempdir, workspace.id)
    }

    /// Create an isolated local ACP client backed by the deterministic Go fixture.
    async fn mock_client() -> (Arc<Client>, Arc<RecordingPermissions>, TempDir) {
        let (client, permissions, tempdir, workspace_id) =
            mock_client_empty(ConversationStore::new(None)).await;
        let session = client
            .create_session("mock", "mock-model", &workspace_id)
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

    /// ACP agent spawn uses process-group isolation so descendants die on
    /// shutdown (kill_on_drop alone only reaps the direct child).
    #[cfg(unix)]
    #[tokio::test]
    async fn agent_process_group_kill_reaps_descendant() {
        use std::process::Command as StdCommand;

        use crate::procutil::{configure_process_group, ProcessGroupCleanup};

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

    /// Rebinding replaces only transport ownership: the stable local ID,
    /// display metadata, and durable transcript must survive intact.
    #[tokio::test]
    async fn rebind_preserves_session_identity_and_event_history() {
        let (client, _permissions, _workspace) = mock_client().await;
        let session = client.list_sessions().pop().expect("one mock session");
        client
            .send_prompt(&session.id, "record this before rebind", &[])
            .await
            .expect("complete first prompt");
        let before = client
            .deps
            .event_bus
            .query(&session.id, 0, 100)
            .await
            .expect("query event history");
        assert!(!before.is_empty(), "prompt should create durable history");

        let rebound = client
            .rebind_session(&session.id, "mock", "mock-model", 8 * 1024)
            .await
            .expect("rebind idle session");
        let after = client
            .deps
            .event_bus
            .query(&session.id, 0, 100)
            .await
            .expect("query event history after rebind");

        assert_eq!(rebound.id, session.id);
        assert_eq!(rebound.name, "Mock session");
        assert!(
            after.len() > before.len(),
            "rebind should preserve history and append a restart event"
        );
        client
            .close_session(&session.id)
            .await
            .expect("close rebound session");
    }

    /// After restart, `list_sessions` must surface durable metadata with no actors.
    #[tokio::test]
    async fn list_sessions_includes_stored_without_live_actors() {
        let store_dir = TempDir::new().expect("store dir");
        let store_path = store_dir.path().join("conversations.json");
        let (client, _permissions, _workspace, workspace_id) =
            mock_client_empty(ConversationStore::new(Some(store_path))).await;
        let now = Utc::now();
        client
            .deps
            .conversation_store
            .persist(&[StoredSession::from_parts(
                SessionInfo {
                    id: "sess-persisted".to_string(),
                    name: "Survived restart".to_string(),
                    // Stale running bit from the previous daemon process.
                    status: "running".to_string(),
                    agent_id: "mock".to_string(),
                    model_id: "mock-model".to_string(),
                    workspace: workspace_id,
                    created_at: now,
                    updated_at: now,
                },
                "acp-prior-1",
            )])
            .expect("seed conversations.json");

        assert!(
            client.list_sessions().is_empty(),
            "store is not visible until load_conversations"
        );
        client
            .load_conversations()
            .expect("load durable conversations");

        let listed = client.list_sessions();
        assert_eq!(listed.len(), 1, "list must include stored session");
        assert_eq!(listed[0].id, "sess-persisted");
        assert_eq!(listed[0].name, "Survived restart");
        assert_eq!(
            listed[0].status, "idle",
            "loaded sessions must be idle until an actor is restored"
        );
        assert!(
            !client
                .has_live_session("sess-persisted")
                .expect("live lookup"),
            "load_conversations must not auto-start actors"
        );
    }

    /// Prompting a stored-only id starts an actor and must not wipe EventBus history.
    #[tokio::test]
    async fn prompt_on_stored_session_starts_actor_without_wiping_history() {
        let store_dir = TempDir::new().expect("store dir");
        let store_path = store_dir.path().join("conversations.json");
        let (client, _permissions, _workspace, workspace_id) =
            mock_client_empty(ConversationStore::new(Some(store_path))).await;
        let now = Utc::now();
        let session_id = "sess-restore-prompt";
        client
            .deps
            .conversation_store
            .persist(&[StoredSession::from_parts(
                SessionInfo {
                    id: session_id.to_string(),
                    name: "Prior chat".to_string(),
                    status: "idle".to_string(),
                    agent_id: "mock".to_string(),
                    model_id: "mock-model".to_string(),
                    workspace: workspace_id,
                    created_at: now,
                    updated_at: now,
                },
                "",
            )])
            .expect("seed conversations.json");
        client
            .load_conversations()
            .expect("load durable conversations");

        let mut prior = Event::new(0, EventType::PromptSubmitted, session_id, now);
        prior.role = "user".to_string();
        prior.content = "history from before restart".to_string();
        client
            .deps
            .event_bus
            .append_and_publish(prior)
            .await
            .expect("seed prior transcript");
        let before = client
            .deps
            .event_bus
            .query(session_id, 0, 100)
            .await
            .expect("query history before restore");
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].content, "history from before restart");

        client
            .send_prompt(session_id, "hello after restart", &[])
            .await
            .expect("prompt must lazily restore the actor");

        assert!(
            client.has_live_session(session_id).expect("live lookup"),
            "prompt should promote the dormant session to a live actor"
        );
        let info = client
            .get_session_info(session_id)
            .expect("restored session info");
        assert_eq!(info.name, "Prior chat");
        assert_eq!(info.id, session_id);

        let after = client
            .deps
            .event_bus
            .query(session_id, 0, 100)
            .await
            .expect("query history after restore");
        assert!(
            after.len() > before.len(),
            "restore prompt should append events without wiping prior history"
        );
        assert_eq!(
            after[0].content, "history from before restart",
            "EventBus history must survive actor restore"
        );

        client
            .close_session(session_id)
            .await
            .expect("close restored session");
    }

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

    /// Durable `acpSessionId` survives load→rename→persist without leaking into REST info.
    #[tokio::test]
    async fn persisted_acp_session_id_survives_rename_round_trip() {
        let store_dir = TempDir::new().expect("store dir");
        let store_path = store_dir.path().join("conversations.json");
        let (client, _permissions, _workspace, workspace_id) =
            mock_client_empty(ConversationStore::new(Some(store_path.clone()))).await;
        let now = Utc::now();
        client
            .deps
            .conversation_store
            .persist(&[StoredSession::from_parts(
                SessionInfo {
                    id: "sess-acp-id".to_string(),
                    name: "With ACP id".to_string(),
                    status: "idle".to_string(),
                    agent_id: "mock".to_string(),
                    model_id: "mock-model".to_string(),
                    workspace: workspace_id,
                    created_at: now,
                    updated_at: now,
                },
                "acp-durable-9",
            )])
            .expect("seed");
        client
            .load_conversations()
            .expect("load durable conversations");

        let info = client
            .get_session_info("sess-acp-id")
            .expect("get session info");
        let info_json = serde_json::to_value(&info).expect("serialize REST info");
        assert!(
            info_json.get("acpSessionId").is_none(),
            "get_session_info must not expose acpSessionId"
        );

        client
            .rename_session("sess-acp-id", "Renamed")
            .expect("rename dormant");
        let reloaded = client
            .deps
            .conversation_store
            .load()
            .expect("reload store after rename");
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].info.name, "Renamed");
        assert_eq!(
            reloaded[0].acp_session_id, "acp-durable-9",
            "rename must preserve durable acpSessionId"
        );
    }

    #[test]
    fn load_session_mcp_servers_attaches_enabled_stdio() {
        use agent_client_protocol::schema::v1::{McpCapabilities, McpServer};
        use std::fs;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(
            &path,
            r#"{
  "version": 1,
  "mcpServers": {
    "echo": {
      "command": "echo",
      "args": ["hi"]
    },
    "remote": {
      "type": "http",
      "url": "https://example.com/mcp",
      "enabled": true
    },
    "off": {
      "command": "false",
      "enabled": false
    }
  }
}"#,
        )
        .unwrap();

        // Default caps: stdio always ok; http/sse off unless advertised.
        let stdio_only = super::load_session_mcp_servers(Some(&path), &McpCapabilities::new());
        assert_eq!(stdio_only.len(), 1);
        assert!(matches!(stdio_only[0], McpServer::Stdio(_)));

        let with_http =
            super::load_session_mcp_servers(Some(&path), &McpCapabilities::new().http(true));
        assert_eq!(with_http.len(), 2);

        // Malformed config must not fail session create.
        fs::write(&path, "{not-json").unwrap();
        assert!(super::load_session_mcp_servers(Some(&path), &McpCapabilities::new()).is_empty());
        assert!(super::load_session_mcp_servers(None, &McpCapabilities::new()).is_empty());
    }

    #[test]
    fn initialize_client_info_is_non_empty() {
        use agent_client_protocol::schema::v1::{Implementation, InitializeRequest};
        use agent_client_protocol::schema::ProtocolVersion;

        let req = InitializeRequest::new(ProtocolVersion::V1).client_info(Implementation::new(
            super::ACP_CLIENT_NAME,
            super::ACP_CLIENT_VERSION,
        ));
        let info = req.client_info.expect("client_info must be set");
        assert!(!info.name.is_empty(), "client name must not be empty");
        assert!(!info.version.is_empty(), "client version must not be empty");
        assert_eq!(info.name, "LocalAgentInterface");
        assert_eq!(info.version, "1.0");
    }
}
