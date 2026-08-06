//! Stable ACP client facade.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex as AsyncMutex;

use super::super::{context::PromptPipeline, AgentRegistry};
use super::registry::SessionRegistry;
use crate::config::{AgentInfo, PromptContextSettings};
use crate::events::SharedEventBus;
use crate::interfaces::{
    ACPClient, AppError, Attachment, PermissionManager, ProviderInfo, Session, SessionInfo,
    WorkspaceManager,
};

/// Constructor-only dependencies for ACP core.
pub struct ClientDeps {
    pub registry: Arc<AgentRegistry>,
    pub workspaces: Arc<dyn WorkspaceManager>,
    pub permissions: Arc<dyn PermissionManager>,
    /// Ordered durable event stream for prompt lifecycle and ACP updates.
    pub event_bus: SharedEventBus,
    /// Optional durable metadata file; `None` is useful for isolated tests.
    pub conversation_store: super::super::store::ConversationStore,
    /// Path to `mcp.json`. `None` skips MCP attachment on session/new (tests).
    pub mcp_config_path: Option<PathBuf>,
    /// Grace period after a cooperative cancel before force-closing the
    /// session. Defaults to 10s if unset. Configurable via `config.toml`
    /// (`cancelGracePeriodSeconds`).
    pub cancel_grace_period: std::time::Duration,
    /// Idle watchdog timeout for `Running` sessions. If no events arrive from
    /// the agent for this duration, the session is marked `Failed` with an
    /// `AgentExited` event. Defaults to 120s if unset. Configurable via
    /// `config.toml` (`agentIdleTimeoutSeconds`).
    pub agent_idle_timeout: std::time::Duration,
}

/// ACP lifecycle facade. The registry lock protects only metadata and command
/// senders; lifecycle operations clone all state before awaiting.
pub struct Client {
    pub(super) deps: ClientDeps,
    pub(super) sessions: SessionRegistry,
    /// Serializes dormant→live restores so concurrent operations cannot spawn twice.
    pub(super) restore_lock: AsyncMutex<()>,
    pub(super) pipeline: Arc<PromptPipeline>,
}

impl Client {
    /// Creates an ACP client with all service dependencies supplied up front.
    #[must_use]
    pub fn new(deps: ClientDeps) -> Self {
        Self {
            deps,
            sessions: SessionRegistry::default(),
            restore_lock: AsyncMutex::new(()),
            pipeline: Arc::new(PromptPipeline::default()),
        }
    }

    /// Returns the frontend context tracker used by prompt middleware.
    #[must_use]
    pub fn open_files_tracker(&self) -> &super::super::context::OpenFilesTracker {
        &self.pipeline.tracker
    }

    /// Snapshot of the loaded profile config (`GET /api/profiles`).
    ///
    /// # Errors
    ///
    /// Returns an error if the profile config cannot be loaded.
    pub fn profile_config(&self) -> Result<super::super::profile_config::ProfileConfig, AppError> {
        self.pipeline.profiles.config()
    }

    /// Replaces in-memory profile config after a validated REST write.
    ///
    /// # Errors
    ///
    /// Returns an error if the replacement is rejected by the config store.
    pub fn replace_profile_config(
        &self,
        config: super::super::profile_config::ProfileConfig,
    ) -> Result<(), AppError> {
        self.pipeline.profiles.replace_config(config)
    }

    /// Replaces the live bounded path-context settings after a durable config write.
    ///
    /// # Errors
    ///
    /// Returns an error if the new settings cannot be applied to the pipeline.
    pub fn replace_prompt_context_settings(
        &self,
        settings: PromptContextSettings,
    ) -> Result<(), AppError> {
        self.pipeline.replace_context_settings(settings)
    }

    /// Creates a session with an optional profile bound before actor startup.
    ///
    /// # Errors
    ///
    /// Returns an error if the session or its profile cannot be created.
    pub async fn create_session_with_profile(
        &self,
        agent_id: &str,
        model_id: &str,
        workspace_id: &str,
        profile_id: Option<&str>,
    ) -> Result<SessionInfo, AppError> {
        self.create_session_with_profile_inner(agent_id, model_id, workspace_id, profile_id)
            .await
    }

    /// Return the retained, bounded stderr tail for a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is unknown or the tail cannot be read.
    pub fn stderr_tail(&self, session_id: &str) -> Result<String, AppError> {
        self.sessions.stderr_tail(session_id)
    }

    /// Read durable metadata without starting any actors.
    ///
    /// # Errors
    ///
    /// Returns an error if durable metadata cannot be read.
    pub fn load_conversation_metadata(&self) -> Result<Vec<SessionInfo>, AppError> {
        self.load_conversation_metadata_inner()
    }

    /// Restore durable metadata into the dormant registry.
    ///
    /// # Errors
    ///
    /// Returns an error if durable metadata cannot be loaded.
    pub fn load_conversations(&self) -> Result<(), AppError> {
        self.load_conversations_inner()
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
        self.sessions.info(session_id)
    }

    fn session_history_capabilities(
        &self,
        session_id: &str,
    ) -> Result<crate::interfaces::SessionHistoryCapabilities, AppError> {
        let caps = self.sessions.history_caps(session_id)?;
        if !self.sessions.contains_live(session_id)? {
            tracing::debug!(
                session_id,
                "session history caps unavailable: agent not live (cold-start deferred to Q8)"
            );
        }
        Ok(caps)
    }

    fn list_sessions(&self) -> Vec<Session> {
        self.sessions.list().unwrap_or_else(|error| {
            tracing::error!(%error, "ACP session registry lock poisoned during list_sessions");
            Vec::new()
        })
    }

    async fn send_prompt(
        &self,
        session_id: &str,
        content: &str,
        attachments: &[Attachment],
    ) -> Result<(), AppError> {
        self.send_prompt_inner(session_id, content, attachments)
            .await
    }

    fn rename_session(&self, session_id: &str, name: &str) -> Result<(), AppError> {
        self.rename_session_inner(session_id, name)
    }

    async fn rebind_session(
        &self,
        session_id: &str,
        agent_id: &str,
        model_id: &str,
        max_transfer_bytes: i64,
    ) -> Result<SessionInfo, AppError> {
        self.rebind_session_inner(session_id, agent_id, model_id, max_transfer_bytes)
            .await
    }

    async fn switch_model(&self, session_id: &str, model_id: &str) -> Result<(), AppError> {
        self.switch_model_inner(session_id, model_id).await
    }

    async fn cancel_session(&self, session_id: &str) -> Result<(), AppError> {
        self.cancel_session_inner(session_id).await
    }

    async fn close_session(&self, session_id: &str) -> Result<(), AppError> {
        self.close_session_inner(session_id).await
    }

    async fn set_session_profile(&self, session_id: &str, profile: &str) -> Result<(), AppError> {
        self.set_session_profile_inner(session_id, profile).await
    }

    async fn list_providers(&self, session_id: &str) -> Result<Vec<ProviderInfo>, AppError> {
        self.list_providers_inner(session_id).await
    }

    async fn set_provider(
        &self,
        session_id: &str,
        id: &str,
        api_type: &str,
        base_url: &str,
        headers: std::collections::HashMap<String, String>,
    ) -> Result<(), AppError> {
        self.set_provider_inner(session_id, id, api_type, base_url, headers)
            .await
    }

    async fn disable_provider(&self, session_id: &str, id: &str) -> Result<(), AppError> {
        self.disable_provider_inner(session_id, id).await
    }
}
