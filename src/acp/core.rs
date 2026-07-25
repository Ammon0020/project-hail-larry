//! ACP session transport and actor lifecycle.
//!
//! The SDK connection is deliberately owned by one task per session. Its
//! `connect_with` closure is the only place a `ConnectionTo<Agent>` is valid,
//! so callers communicate with that task through a bounded command channel
//! rather than attempting to store an SDK connection in the session registry.

mod actor;
mod diagnostics;
mod events;
mod handlers;
mod mcp;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, EmbeddedResource, EmbeddedResourceResource, PromptRequest,
    SessionId, TextContent, TextResourceContents,
};
use agent_client_protocol::{Agent, ConnectionTo};
use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};
use uuid::Uuid;

use super::providers::{
    require_providers_supported, rpc_disable_provider, rpc_list_providers, rpc_set_model_config,
    rpc_set_profile_config, rpc_set_provider, SessionCaps, MODEL_SWITCH_UNSUPPORTED_MSG,
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
    ACPClient, AppError, Attachment, EventPayload, PermissionManager, ProviderInfo, Session,
    SessionInfo, WorkspaceInfo, WorkspaceManager,
};

use actor::{ActorCommand, ActorExit, ACTOR_COMMAND_CAPACITY};
use diagnostics::StderrTail;
use events::append_payload;

/// Maximum retained agent stderr diagnostic tail. Agent stderr is untrusted and
/// must never be allowed to grow the daemon's memory without bound.
pub const STDERR_TAIL_BYTES: usize = 8 * 1024;

/// Maximum concurrent live ACP sessions. Each session pins an agent child
/// process, so a cap prevents unbounded process-exhaustion DoS.
const MAX_SESSIONS: usize = 32;
/// Safe default for a model-switch rebind transfer (256 KiB).
const MODEL_SWITCH_TRANSFER_BYTES: i64 = 256 * 1024;

/// Grace period after a cooperative cancel before force-closing the session.
/// A malicious agent can ignore the cancel notification; after this timeout the
/// session is force-closed to kill the agent process and abort in-flight callbacks.
const CANCEL_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(10);

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
    actor: actor::Handle,
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
    /// Config option id for the mode/profile selector when the agent advertises
    /// one. `None` means the agent lacks the capability; profile instructions
    /// are injected into the prompt context as the fallback (context.rs).
    profile_config_id: Option<String>,
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

    /// Snapshot of the loaded profile config (`GET /api/profiles`).
    pub fn profile_config(&self) -> Result<super::profile_config::ProfileConfig, AppError> {
        self.pipeline.profiles.config()
    }

    /// Replaces in-memory profile config after a validated REST write.
    pub fn replace_profile_config(
        &self,
        config: super::profile_config::ProfileConfig,
    ) -> Result<(), AppError> {
        self.pipeline.profiles.replace_config(config)
    }

    /// Creates a session with an optional profile bound before actor startup.
    ///
    /// ACP receives inline MCP servers only during `session/new`/`session/load`,
    /// so applying a profile after the actor starts would attach the wrong
    /// server set. Unknown supplied profile ids are rejected rather than
    /// normalized to the default.
    pub async fn create_session_with_profile(
        &self,
        agent_id: &str,
        model_id: &str,
        workspace_id: &str,
        profile_id: Option<&str>,
    ) -> Result<SessionInfo, AppError> {
        let config = self.pipeline.profiles.config()?;
        let selected_profile = match profile_id {
            Some(profile) => {
                let trimmed = profile.trim();
                if trimmed.is_empty() {
                    return Err(AppError::validation("profile id must not be empty"));
                }
                if !config
                    .profiles
                    .keys()
                    .any(|id| id.eq_ignore_ascii_case(trimmed))
                {
                    return Err(AppError::validation(format!(
                        "unknown profile id: {profile}"
                    )));
                }
                config.normalize_profile_id(trimmed)
            }
            None => config.default_profile_id.clone(),
        };
        if let Some(path) = self.deps.mcp_config_path.as_deref() {
            match crate::mcp::File::load(path) {
                Ok(file) => config
                    .validate_profile_mcp_servers_against(
                        &selected_profile,
                        file.mcp_servers.keys().map(String::as_str),
                    )
                    .map_err(|error| AppError::validation(error.to_string()))?,
                // MCP configuration is additive. A malformed or inaccessible
                // file is handled by session setup as an empty server list.
                Err(error) => tracing::warn!(
                    path = %path.display(),
                    %error,
                    "skipping profile MCP server-name validation because mcp config is unavailable"
                ),
            }
        }

        // Validate early so a bad agent/workspace/profile fails before allocating an id.
        let _agent = self
            .deps
            .registry
            .resolve(agent_id, model_id)
            .map_err(AppError::validation)?;
        let _workspace = self.resolve_workspace(workspace_id).await?;
        let id = format!("sess-{}", Uuid::new_v4().simple());
        self.pipeline.profiles.set_profile(&id, &selected_profile)?;
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
        let published = match self.register_live_session(info, String::new()).await {
            Ok(session) => session,
            Err(error) => {
                self.pipeline.profiles.clear(&id);
                return Err(error);
            }
        };
        if let Err(error) = self.persist_sessions() {
            // A failed create must not leave a live, invisible session behind.
            tracing::error!(session_id = %published.id, error = %error, "failed to persist new ACP session");
            let _ = self.close_session(&published.id).await;
            return Err(error);
        }
        Ok(published)
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
        persist_sessions_to(&live, &dormant, &self.deps.conversation_store)
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
        // Cap concurrent live sessions to prevent process-exhaustion DoS.
        if self.sessions_read()?.len() >= MAX_SESSIONS {
            return Err(AppError::RateLimited(
                "too many concurrent ACP sessions".to_string(),
            ));
        }
        let agent = self
            .deps
            .registry
            .resolve(&info.agent_id, &info.model_id)
            .map_err(AppError::validation)?;
        let workspace = self.resolve_workspace(&info.workspace).await?;
        let workspace_path = PathBuf::from(workspace.path);
        let id = info.id.clone();
        let stderr_tail = Arc::new(Mutex::new(StderrTail::default()));
        let prompt_cancel = Arc::new(AtomicBool::new(false));
        let spawned = actor::spawn(
            actor::Config {
                local_session_id: id.clone(),
                agent,
                workspace_id: info.workspace.clone(),
                workspace_path,
                permissions: Arc::clone(&self.deps.permissions),
                workspaces: Arc::clone(&self.deps.workspaces),
                stderr_tail: Arc::clone(&stderr_tail),
                event_bus: Arc::clone(&self.deps.event_bus),
                prompt_cancel: Arc::clone(&prompt_cancel),
                mcp_config_path: self.deps.mcp_config_path.clone(),
                profiles: Arc::clone(&self.pipeline.profiles),
                persisted_acp_session_id,
            },
            ACTOR_COMMAND_CAPACITY,
        );

        let result = spawned
            .ready
            .await
            .map_err(|_| AppError::internal("ACP session actor exited during startup"))?;
        let startup = result?;
        let mut entry = SessionEntry {
            info,
            state: SessionState::Created,
            actor: spawned.handle.clone(),
            stderr_tail,
            prompt_cancel,
            caps: startup.caps,
            model_config_id: startup.model_config_id,
            profile_config_id: startup.profile_config_id,
            acp_session_id: startup.acp_session_id,
        };
        entry.apply_state(SessionState::Idle);
        let published = entry.info.clone();
        self.sessions_write()?.insert(id, entry);
        let _ = spawned.registered.send(());
        self.watch_actor_terminal(spawned.terminal, spawned.handle, published.id.clone());
        Ok(published)
    }

    /// Consume actor loss outside the registry lock. A replacement actor gets a
    /// new opaque handle, so an old actor cannot fail the rebound session.
    fn watch_actor_terminal(
        &self,
        terminal: oneshot::Receiver<actor::TerminalOutcome>,
        handle: actor::Handle,
        session_id: String,
    ) {
        let sessions = Arc::clone(&self.sessions);
        let permissions = Arc::clone(&self.deps.permissions);
        let event_bus = Arc::clone(&self.deps.event_bus);
        tokio::spawn(async move {
            let Ok(actor::TerminalOutcome::Failed(error)) = terminal.await else {
                return;
            };
            let current = sessions
                .write()
                .ok()
                .and_then(|mut entries| {
                    entries.get_mut(&session_id).and_then(|entry| {
                        (entry.actor.id() == handle.id()).then(|| {
                            entry.apply_state(SessionState::Failed);
                        })
                    })
                })
                .is_some();
            if !current {
                return;
            }
            permissions.clear_session(&session_id);
            if let Err(append_error) = append_payload(
                &event_bus,
                &session_id,
                EventPayload::AgentExited {
                    content: "ACP session actor exited unexpectedly".to_string(),
                },
            )
            .await
            {
                tracing::error!(session_id, error = %append_error, "failed to persist ACP actor-exit event");
            }
            tracing::warn!(session_id, error = %error, "ACP session actor ended");
        });
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
            _ => Ok((entry.actor.commands(), Arc::clone(&entry.prompt_cancel))),
        })
    }

    /// Look up a session's command sender and cached initialize capabilities.
    fn session_for_providers(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<ActorCommand>, SessionCaps), AppError> {
        self.map_live_session(session_id, |entry| Ok((entry.actor.commands(), entry.caps)))
    }

    /// Look up command sender + model config id for a live model switch.
    fn session_for_model_switch(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<ActorCommand>, Option<String>), AppError> {
        self.map_live_session(session_id, |entry| {
            Ok((entry.actor.commands(), entry.model_config_id.clone()))
        })
    }

    /// Look up command sender + profile config id for a live profile switch.
    ///
    /// Returns `Option<String>` so the caller can decide whether to send the
    /// `SetProfile` actor command (capability gate) or rely on the
    /// prompt-injection fallback in `context.rs`.
    fn session_for_profile_switch(
        &self,
        session_id: &str,
    ) -> Result<(mpsc::Sender<ActorCommand>, Option<String>), AppError> {
        self.map_live_session(session_id, |entry| {
            Ok((entry.actor.commands(), entry.profile_config_id.clone()))
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
                    entry.actor.commands(),
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
        self.create_session_with_profile(agent_id, model_id, workspace_id, None)
            .await
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
                entry.actor.commands(),
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
        let spawned = actor::spawn(
            actor::Config {
                local_session_id: session_id.to_string(),
                agent,
                workspace_id: workspace_id.clone(),
                workspace_path: PathBuf::from(workspace.path),
                permissions: Arc::clone(&self.deps.permissions),
                workspaces: Arc::clone(&self.deps.workspaces),
                stderr_tail: Arc::clone(&stderr_tail),
                event_bus: Arc::clone(&self.deps.event_bus),
                prompt_cancel: Arc::clone(&prompt_cancel),
                mcp_config_path: self.deps.mcp_config_path.clone(),
                profiles: Arc::clone(&self.pipeline.profiles),
                // Rebind switches agents; clear the prior ACP id so we never
                // attempt session/load against the wrong agent (Go parity).
                persisted_acp_session_id: String::new(),
            },
            ACTOR_COMMAND_CAPACITY,
        );
        let startup = match spawned.ready.await {
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
            entry.actor = spawned.handle.clone();
            entry.stderr_tail = stderr_tail;
            entry.prompt_cancel = prompt_cancel;
            entry.caps = startup.caps;
            entry.model_config_id = startup.model_config_id;
            // Replacement agent may advertise a different mode option (or none).
            entry.profile_config_id = startup.profile_config_id;
            entry.acp_session_id = startup.acp_session_id;
            entry.info.agent_id = agent_id.to_string();
            entry.info.model_id = model_id.to_string();
            entry.apply_state(SessionState::Idle);
            entry.info.clone()
        };
        // The replacement actor has initialized, and the entry now owns its
        // sender, so it may safely start receiving commands.
        let _ = spawned.registered.send(());
        self.watch_actor_terminal(spawned.terminal, spawned.handle, session_id.to_string());
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

        // Security: cancel is cooperative — a malicious agent can ignore the
        // notification and keep its process alive. Spawn a grace-period watchdog
        // that force-closes the session (killing the process group) if the agent
        // has not acknowledged within CANCEL_GRACE_PERIOD. Removing the registry
        // entry first makes this idempotent against a concurrent close_session.
        let sessions = Arc::clone(&self.sessions);
        let dormant = Arc::clone(&self.dormant);
        let permissions = Arc::clone(&self.deps.permissions);
        let conversation_store = self.deps.conversation_store.clone();
        let pipeline = Arc::clone(&self.pipeline);
        let session_id_owned = session_id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(CANCEL_GRACE_PERIOD).await;
            // Only escalate if the session is still live and interrupted — if the
            // agent acknowledged or the user already closed, leave it alone.
            let sender = {
                let Ok(sessions) = sessions.read() else {
                    return;
                };
                let Some(entry) = sessions.get(&session_id_owned) else {
                    return;
                };
                if entry.state != SessionState::Interrupted {
                    return;
                }
                entry.actor.commands()
            };
            // Remove first to make the force-close idempotent (mirrors close_session).
            let removed = sessions
                .write()
                .ok()
                .and_then(|mut s| s.remove(&session_id_owned));
            if removed.is_none() {
                return;
            }
            let _ = dormant
                .write()
                .ok()
                .and_then(|mut s| s.remove(&session_id_owned));
            if let (Ok(live), Ok(dormant)) = (sessions.read(), dormant.read()) {
                let _ = persist_sessions_to(&live, &dormant, &conversation_store);
            }
            permissions.clear_session(&session_id_owned);
            let (closed_tx, closed_rx) = oneshot::channel();
            if sender.send(ActorCommand::Close(closed_tx)).await.is_ok() {
                let _ = closed_rx.await;
            }
            pipeline.clear(&session_id_owned);
            tracing::info!(
                session_id = %session_id_owned,
                "force-closed ACP session after cancel grace period expired"
            );
        });
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
            .actor
            .commands()
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

    /// Set the session's active profile and push it to the agent when supported.
    ///
    /// Capability gate: when the agent advertised a mode-category config option
    /// (cached as `SessionEntry::profile_config_id`), the new profile id is sent
    /// over ACP via `session/set_config_option` *before* local middleware is
    /// updated. Agents without the capability keep using the prompt-injection
    /// fallback in `context.rs` — local state is committed immediately so the
    /// next prompt still injects the selected instructions.
    ///
    /// Profile id is validated against the loaded config; unknown ids return
    /// `AppError::validation` (HTTP 400). A missing session returns
    /// `AppError::not_found` (HTTP 404). On a live capability RPC failure the
    /// local profile is left unchanged so client and agent stay consistent.
    async fn set_session_profile(&self, session_id: &str, profile: &str) -> Result<(), AppError> {
        // Validate the profile id against the loaded config BEFORE storing or
        // sending. `normalize_profile_id` maps unknown ids to the default, so
        // validate the original (case-insensitive) input directly against the
        // config's known keys; unknown ids must surface as 400 rather than
        // silently normalizing to the default.
        let config = self.pipeline.profiles.config()?;
        let trimmed = profile.trim();
        if trimmed.is_empty() {
            return Err(AppError::validation("profile id is required"));
        }
        let known = config
            .profiles
            .keys()
            .any(|id| id.eq_ignore_ascii_case(trimmed));
        if !known {
            return Err(AppError::validation(format!(
                "unknown profile id: {profile}"
            )));
        }
        let normalized = config.normalize_profile_id(profile);

        // Reject missing sessions before any mutation so a stale id does not
        // leave an orphaned entry in the profile map. Live and dormant sessions
        // are both accepted; dormant sessions pick up the profile on actor
        // startup (initial set_config_option send).
        if !self.has_live_session(session_id)? && !self.dormant_read()?.contains_key(session_id) {
            return Err(AppError::not_found_id("session", session_id));
        }

        // Live agents with a mode-category option must confirm first. Commit
        // local state only after the RPC succeeds (or when no RPC is needed).
        let push_result = self.session_for_profile_switch(session_id);
        if let Ok((sender, Some(config_id))) = push_result {
            let (result_tx, result_rx) = oneshot::channel();
            sender
                .send(ActorCommand::SetProfile {
                    config_id,
                    profile_id: normalized.clone(),
                    result: result_tx,
                })
                .await
                .map_err(|_| AppError::internal("ACP session actor is unavailable"))?;
            result_rx
                .await
                .map_err(|_| AppError::internal("ACP set_profile actor exited"))??;
        } else if let Err(error) = push_result {
            // Session not live: local commit below still applies for the next
            // prompt; initial set_config_option on actor startup picks it up.
            tracing::debug!(
                session_id,
                profile = %normalized,
                error = %error,
                "set_session_profile: session not live; profile stored for next prompt"
            );
        }

        // Commit after a successful RPC, or immediately when there is no live
        // capability path (no option / dormant) so prompt injection sees it.
        self.pipeline.profiles.set_profile(session_id, profile)?;
        Ok(())
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

/// Merge live + dormant sessions and persist to durable storage.
/// Live metadata wins for shared ids; dormant fills the rest so a
/// create/rename cannot wipe conversations that have not been restored.
fn persist_sessions_to(
    live: &HashMap<String, SessionEntry>,
    dormant: &HashMap<String, StoredSession>,
    conversation_store: &ConversationStore,
) -> Result<(), AppError> {
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
    conversation_store.persist(&sessions)
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
        ActorCommand::SetProfile {
            config_id,
            profile_id,
            result,
        } => {
            let _ = result
                .send(rpc_set_profile_config(cx, agent_session_id, &config_id, &profile_id).await);
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
                            handle_non_prompt_command(&cx, &agent_session_id, other)
                                .await
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use tempfile::TempDir;

    use super::{AgentRegistry, Client, ClientDeps, ConversationStore, StoredSession};
    use crate::config::{AgentInfo, AgentModel};
    use crate::events::{EventBus, Store};
    use crate::interfaces::{
        ACPClient, AppError, Event, EventStore, EventType, PermissionDecision, PermissionManager,
        PermissionRequest, SessionInfo, WorkspaceManager,
    };
    use crate::workspace::Manager as WorkspaceRegistry;
    use chrono::Utc;

    /// Resolve the mockagent binary path. CI builds the Go mockagent to a
    /// platform-specific location and points `LOCAL_AGENT_MOCKAGENT_BIN` at it
    /// (Windows needs a `.exe` suffix); the default `/tmp/mockagent` matches
    /// the local-dev build documented in the assertion below.
    fn mockagent_bin() -> String {
        std::env::var("LOCAL_AGENT_MOCKAGENT_BIN").unwrap_or_else(|_| "/tmp/mockagent".to_string())
    }

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
    ///
    /// Registers both a default mock agent (mode/profile capability on) and a
    /// `mock-nocap` agent that suppresses the mode option via
    /// `MOCKAGENT_NO_MODE_CAP=1` for rebind/fallback coverage.
    async fn mock_client_empty(
        conversation_store: ConversationStore,
    ) -> (Arc<Client>, Arc<RecordingPermissions>, TempDir, String) {
        let mockagent_bin = mockagent_bin();
        assert!(
            Path::new(&mockagent_bin).exists(),
            "mockagent binary missing at {mockagent_bin}; build it with `go build -o /tmp/mockagent ./cmd/mockagent/` or set LOCAL_AGENT_MOCKAGENT_BIN"
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
        let mock_model = AgentModel::new("mock-model".to_string(), "Mock model".to_string());
        let registry = Arc::new(AgentRegistry::from_agents([
            AgentInfo {
                id: "mock".to_string(),
                name: "Mock agent".to_string(),
                command: mockagent_bin.clone(),
                args: Vec::new(),
                models: vec![mock_model.clone()],
                warning: String::new(),
            },
            // `env` injects MOCKAGENT_NO_MODE_CAP without process-global set_var.
            AgentInfo {
                id: "mock-nocap".to_string(),
                name: "Mock agent without mode cap".to_string(),
                command: "env".to_string(),
                args: vec!["MOCKAGENT_NO_MODE_CAP=1".to_string(), mockagent_bin.clone()],
                models: vec![mock_model],
                warning: String::new(),
            },
        ]));
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

    /// Startup failure (nonexistent agent command) must return an error and
    /// leave no entry in the live session registry.
    #[tokio::test]
    async fn startup_failure_before_publication_is_not_registered() {
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
            id: "broken".to_string(),
            name: "Broken agent".to_string(),
            command: "/nonexistent/binary/that/does/not/exist".to_string(),
            args: Vec::new(),
            models: vec![AgentModel::new("m".to_string(), "M".to_string())],
            warning: String::new(),
        }]));
        let client = Arc::new(Client::new(ClientDeps {
            registry,
            workspaces,
            permissions: permissions.clone(),
            event_bus,
            conversation_store: ConversationStore::new(None),
            mcp_config_path: None,
        }));

        let error = client
            .create_session("broken", "m", &workspace.id)
            .await
            .expect_err("startup with a nonexistent agent must fail");
        assert!(
            error.to_string().to_ascii_lowercase().contains("spawn"),
            "startup error must mention spawn failure: {error}"
        );
        assert!(
            client.list_sessions().is_empty(),
            "failed startup must not publish a live session"
        );
    }

    /// Unexpected post-startup exit (agent crashes after initialize + new)
    /// must transition the session to Failed and append an AgentExited event.
    #[tokio::test]
    async fn unexpected_post_startup_exit_marks_session_failed() {
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
        // The env wrapper injects MOCKAGENT_EXIT_AFTER_INIT so the mock exits
        // right after session/new, simulating an unexpected post-startup crash.
        let registry = Arc::new(AgentRegistry::from_agents([AgentInfo {
            id: "mock-exit".to_string(),
            name: "Mock agent that exits after init".to_string(),
            command: "env".to_string(),
            args: vec!["MOCKAGENT_EXIT_AFTER_INIT=1".to_string(), mockagent_bin()],
            models: vec![AgentModel::new(
                "mock-model".to_string(),
                "Mock model".to_string(),
            )],
            warning: String::new(),
        }]));
        let client = Arc::new(Client::new(ClientDeps {
            registry,
            workspaces,
            permissions: permissions.clone(),
            event_bus: event_bus.clone(),
            conversation_store: ConversationStore::new(None),
            mcp_config_path: None,
        }));

        // create_session may succeed (readiness fires before the crash) or
        // fail (if the SDK cancels the closure before registration). Either
        // way, the terminal watcher must converge the session to Failed.
        let session_id = match client
            .create_session("mock-exit", "mock-model", &workspace.id)
            .await
        {
            Ok(session) => session.id,
            Err(_) => {
                // Startup failure path: the session was never published, so
                // there is nothing to transition. This is also acceptable.
                return;
            }
        };

        // Wait for the terminal watcher to mark the session Failed.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if client
                    .get_session_info(&session_id)
                    .map(|s| s.status == "failed")
                    .unwrap_or(false)
                {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("session must transition to failed after unexpected actor exit");

        // An AgentExited event must be appended to the durable store.
        let events = event_bus
            .query(&session_id, 0, 1000)
            .await
            .expect("query session events");
        assert!(
            events
                .iter()
                .any(|e| e.event_type == EventType::AgentExited),
            "unexpected actor exit must append an AgentExited event"
        );
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

    /// Rebind must refresh `profile_config_id` from the replacement actor so
    /// `session_for_profile_switch` reflects the new agent's mode capability.
    #[tokio::test]
    async fn rebind_refreshes_profile_config_id_from_replacement_agent() {
        let (client, _permissions, _workspace, workspace_id) =
            mock_client_empty(ConversationStore::new(None)).await;
        let session = client
            .create_session("mock-nocap", "mock-model", &workspace_id)
            .await
            .expect("create session without mode capability");

        let (_, before_cfg) = client
            .session_for_profile_switch(&session.id)
            .expect("live session lookup");
        assert_eq!(
            before_cfg, None,
            "mock-nocap must not advertise a mode/profile config option"
        );

        client
            .rebind_session(&session.id, "mock", "mock-model", 8 * 1024)
            .await
            .expect("rebind to agent with mode capability");

        let (_, after_cfg) = client
            .session_for_profile_switch(&session.id)
            .expect("live session lookup after rebind");
        assert_eq!(
            after_cfg.as_deref(),
            Some("profile"),
            "rebind must cache the replacement agent's profile config id"
        );

        client
            .close_session(&session.id)
            .await
            .expect("close rebound session");
    }

    /// When a live profile RPC cannot be delivered, local middleware must not
    /// advance — client and agent stay consistent (commit-after-RPC order).
    #[tokio::test]
    async fn set_session_profile_leaves_local_state_on_rpc_failure() {
        let (client, _permissions, _workspace) = mock_client().await;
        let session = client.list_sessions().pop().expect("one mock session");

        // Ensure a known local selection before the failed switch.
        client
            .pipeline
            .profiles
            .set_profile(&session.id, "code")
            .expect("seed local profile");
        assert_eq!(
            client
                .pipeline
                .profiles
                .profile(&session.id)
                .expect("read seeded profile"),
            "code"
        );

        // Force the capability path, then break the actor command channel so
        // SetProfile cannot complete. Local state must stay at "code".
        {
            let mut sessions = client.sessions.write().expect("sessions lock");
            let entry = sessions
                .get_mut(&session.id)
                .expect("session remains registered");
            entry.profile_config_id = Some("profile".to_string());
            entry.actor = super::actor::Handle::dead();
        }

        let result = client.set_session_profile(&session.id, "ask").await;
        assert!(
            result.is_err(),
            "broken actor channel must surface as set_session_profile error"
        );
        assert_eq!(
            client
                .pipeline
                .profiles
                .profile(&session.id)
                .expect("read profile after failed switch"),
            "code",
            "local profile must not commit when the ACP update fails"
        );
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
}
