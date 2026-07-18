//! Shared service traits at real replacement / test boundaries.
//!
//! Per the epic and S-INTERFACES story: use traits only where an alternate
//! implementation is expected (tests, future backends), not for every Go
//! interface by default. `AppState` may hold concrete services when no alternate
//! is needed.
//!
//! All service dependencies are constructor arguments — no post-construction
//! `Set*` callbacks. [`EventPublisher`] is a narrow dependency for durable
//! app-event publication only (not a general command bus).
//!
//! Async traits use `async_trait` so implementors can use `.await` without
//! boxing futures manually at each call site.

use async_trait::async_trait;
use std::collections::HashMap;

use super::error::AppError;
use super::types::{
    AgentInfo, Attachment, Event, FileNode, PermissionDecision, PermissionRequest, ProviderInfo,
    SearchOptions, SearchResult, Session, SessionInfo, WorkspaceInfo,
};

/// Contract for the append-only event persistence layer (Go `EventStore`).
///
/// Implemented by the `events` package. Append assigns the durable monotonic ID
/// before any publisher makes the event visible to subscribers.
#[async_trait]
pub trait EventStore: Send + Sync {
    /// Append an event to the log. Returns the event with its assigned ID.
    async fn append(&self, event: Event) -> Result<Event, AppError>;

    /// Retrieve events for a session, optionally filtered by cursor (`after_id`)
    /// for reconnection sync.
    async fn query(
        &self,
        session_id: &str,
        after_id: i64,
        limit: i32,
    ) -> Result<Vec<Event>, AppError>;

    /// Retrieve events across all sessions (initial load / global replay).
    async fn query_all(&self, after_id: i64, limit: i32) -> Result<Vec<Event>, AppError>;
}

/// Narrow dependency for durable app-event publication.
///
/// Not a general callback or command bus: callers persist first (via
/// [`EventStore`]), then publish so subscribers see a durable event. The sync
/// handoff is subscribe → replay → dedupe by ID → live delivery.
#[async_trait]
pub trait EventPublisher: Send + Sync {
    /// Publish a previously-persisted event to live subscribers.
    async fn publish(&self, event: &Event) -> Result<(), AppError>;
}

/// Workspace operations (Go `WorkspaceManager`).
#[async_trait]
pub trait WorkspaceManager: Send + Sync {
    /// Register a directory as a workspace.
    async fn register(&self, path: &str) -> Result<WorkspaceInfo, AppError>;

    /// List all registered workspaces.
    async fn list(&self) -> Result<Vec<WorkspaceInfo>, AppError>;

    /// Remove a workspace from the registry by ID.
    async fn remove(&self, id: &str) -> Result<(), AppError>;

    /// Return the file tree for a workspace.
    async fn file_tree(&self, workspace_id: &str) -> Result<Vec<FileNode>, AppError>;

    /// Read a file: content, current revision, binary flag, previewable flag.
    async fn read_file(
        &self,
        workspace_id: &str,
        rel_path: &str,
    ) -> Result<ReadFileResult, AppError>;

    /// Absolute filesystem path for a file after path/symlink validation.
    async fn file_path(&self, workspace_id: &str, rel_path: &str) -> Result<String, AppError>;

    /// Write text content with optimistic revision checking; returns new revision.
    async fn write_file(
        &self,
        workspace_id: &str,
        rel_path: &str,
        content: &str,
        expected_revision: i64,
    ) -> Result<i64, AppError>;

    /// Delete a file or empty directory. Non-empty directories are rejected.
    async fn delete_path(&self, workspace_id: &str, rel_path: &str) -> Result<(), AppError>;

    /// Rename/move a path within the workspace. Fails if the destination exists.
    async fn rename_path(&self, workspace_id: &str, from: &str, to: &str) -> Result<(), AppError>;

    /// Create a directory (and parents as needed). Idempotent if it already
    /// exists as a directory; conflicts if the path exists as a file.
    async fn mkdir(&self, workspace_id: &str, rel_path: &str) -> Result<(), AppError>;

    /// Workspace-wide content search. DTOs live in `interfaces::types` so this
    /// trait does not depend on the search implementation.
    async fn search(
        &self,
        workspace_id: &str,
        pattern: &str,
        opts: SearchOptions,
    ) -> Result<Vec<SearchResult>, AppError>;
}

/// Result of [`WorkspaceManager::read_file`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFileResult {
    pub content: String,
    pub revision: i64,
    pub is_binary: bool,
    pub previewable: bool,
}

/// Callbacks the ACP client uses to notify the daemon of events.
///
/// Implemented by the daemon / composition root to persist + broadcast.
/// Prefer injecting an [`EventPublisher`] (or a concrete service) at construction
/// over a post-construction setter.
pub trait ACPCallbacks: Send + Sync {
    /// Handle a newly produced event (persist + publish).
    fn on_event(&self, event: Event);
}

/// Contract for communicating with AI agents (Go `ACPClient`).
///
/// Implemented by the `acp` package. Synchronous registry methods match Go's
/// in-memory map access; session transport methods are async.
#[async_trait]
pub trait ACPClient: Send + Sync {
    /// List registered agent harnesses and their models.
    async fn list_agents(&self) -> Result<Vec<AgentInfo>, AppError>;

    /// Add an agent to the registry.
    fn register_agent(&self, agent: AgentInfo);

    /// Remove an agent from the registry.
    fn remove_agent(&self, id: &str);

    /// Start a new agent session.
    async fn create_session(
        &self,
        agent_id: &str,
        model_id: &str,
        workspace_id: &str,
    ) -> Result<SessionInfo, AppError>;

    /// Metadata for a single session by ID.
    fn get_session_info(&self, session_id: &str) -> Result<SessionInfo, AppError>;

    /// Live `initialize` session-history caps (list/load/resume). Auth consumers
    /// use this for BROWSE/OPEN/FALLBACK gates. Does not cold-start agents.
    fn session_history_capabilities(
        &self,
        session_id: &str,
    ) -> Result<crate::interfaces::SessionHistoryCapabilities, AppError>;

    /// All conversations, newest activity first.
    fn list_sessions(&self) -> Vec<Session>;

    /// Send a user prompt; responses arrive via ACP callbacks / event publisher.
    async fn send_prompt(
        &self,
        session_id: &str,
        content: &str,
        attachments: &[Attachment],
    ) -> Result<(), AppError>;

    /// Change a conversation's display name.
    fn rename_session(&self, session_id: &str, name: &str) -> Result<(), AppError>;

    /// Switch agent/model while preserving session id and history.
    async fn rebind_session(
        &self,
        session_id: &str,
        agent_id: &str,
        model_id: &str,
        max_transfer_bytes: i64,
    ) -> Result<SessionInfo, AppError>;

    /// Change the model on a live session without restarting the agent process.
    async fn switch_model(&self, session_id: &str, model_id: &str) -> Result<(), AppError>;

    /// Interrupt a running session.
    async fn cancel_session(&self, session_id: &str) -> Result<(), AppError>;

    /// Close a session.
    async fn close_session(&self, session_id: &str) -> Result<(), AppError>;

    /// Set the user's selected profile (Code/Ask/Plan) for a session.
    fn set_session_profile(&self, session_id: &str, profile: &str);

    /// Agent's configurable LLM providers for the session.
    async fn list_providers(&self, session_id: &str) -> Result<Vec<ProviderInfo>, AppError>;

    /// Configure a single LLM provider (headers optional, e.g. authorization).
    async fn set_provider(
        &self,
        session_id: &str,
        id: &str,
        api_type: &str,
        base_url: &str,
        headers: HashMap<String, String>,
    ) -> Result<(), AppError>;

    /// Disable an LLM provider. Callers must check the Required flag first.
    async fn disable_provider(&self, session_id: &str, id: &str) -> Result<(), AppError>;
}

/// Permission handling contract (Go `PermissionManager`).
///
/// Rust design: no post-construction `SetCallback`. The composition root injects
/// an [`EventPublisher`] (or concrete notifications service) at construction so
/// new permission requests are published without a mutable callback slot.
#[async_trait]
pub trait PermissionManager: Send + Sync {
    /// Broadcast a permission prompt; blocks until a decision or cancellation.
    async fn request(&self, req: PermissionRequest) -> Result<PermissionDecision, AppError>;

    /// Record a decision from a device. First response wins.
    async fn respond(&self, request_id: &str, decision: PermissionDecision)
        -> Result<(), AppError>;

    /// Drop all cached permission policies for the session.
    fn clear_session(&self, session_id: &str);

    /// Currently pending permission requests (for re-presentation on reconnect).
    fn get_pending(&self) -> Vec<PermissionRequest>;
}

/// File revision tracking and merge contract (Go `FileSync`).
#[async_trait]
pub trait FileSync: Send + Sync {
    /// Write file content with optimistic locking via `expected_revision`.
    /// Returns the new revision, or [`AppError::StaleRevision`] on conflict.
    async fn save(
        &self,
        workspace_id: &str,
        rel_path: &str,
        content: &str,
        expected_revision: i64,
    ) -> Result<i64, AppError>;

    /// Latest revision of a file.
    async fn current_revision(&self, workspace_id: &str, rel_path: &str) -> Result<i64, AppError>;
}
