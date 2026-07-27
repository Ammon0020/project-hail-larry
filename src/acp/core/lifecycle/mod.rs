//! ACP actor lifecycle orchestration.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

use super::actor::{self, ActorCommand, ACTOR_COMMAND_CAPACITY};
use super::diagnostics::StderrTail;
use super::events::append_payload;
use super::registry::{SessionEntry, SessionState};
use super::{Client, CANCEL_GRACE_PERIOD, MAX_SESSIONS};
use crate::interfaces::{AppError, EventPayload, SessionInfo, WorkspaceInfo};

impl Client {
    pub(super) async fn resolve_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<WorkspaceInfo, AppError> {
        self.deps
            .workspaces
            .list()
            .await?
            .into_iter()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(|| AppError::not_found_id("workspace", workspace_id))
    }

    pub(super) fn load_conversation_metadata_inner(&self) -> Result<Vec<SessionInfo>, AppError> {
        Ok(self
            .deps
            .conversation_store
            .load()?
            .into_iter()
            .map(|stored| stored.to_info())
            .collect())
    }

    pub(super) fn load_conversations_inner(&self) -> Result<(), AppError> {
        let count = self
            .sessions
            .load_dormant(self.deps.conversation_store.load()?)?;
        tracing::info!(
            count,
            "loaded persisted ACP conversations; actors deferred until use"
        );
        Ok(())
    }

    pub(super) fn persist_sessions(&self) -> Result<(), AppError> {
        self.sessions.persist(&self.deps.conversation_store)
    }

    pub(super) fn has_live_session(&self, session_id: &str) -> Result<bool, AppError> {
        self.sessions.contains_live(session_id)
    }

    /// Restore only once while retaining dormant metadata until publication succeeds.
    pub(super) async fn ensure_live_session(&self, session_id: &str) -> Result<(), AppError> {
        if self.has_live_session(session_id)? {
            return Ok(());
        }
        let _guard = self.restore_lock.lock().await;
        if self.has_live_session(session_id)? {
            return Ok(());
        }
        let stored = self.sessions.dormant(session_id)?;
        self.register_live_session(stored.info.clone(), stored.acp_session_id)
            .await?;
        self.sessions.finish_promotion(session_id)?;
        self.persist_sessions()?;
        tracing::info!(session_id, "ACP session actor restored");
        Ok(())
    }

    /// Spawn, wait for readiness, then make the actor available in the registry.
    pub(super) async fn register_live_session(
        &self,
        info: SessionInfo,
        persisted_acp_session_id: String,
    ) -> Result<SessionInfo, AppError> {
        if self.sessions.live_len()? >= MAX_SESSIONS {
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
        let id = info.id.clone();
        let stderr_tail = Arc::new(Mutex::new(StderrTail::default()));
        let prompt_cancel = Arc::new(AtomicBool::new(false));
        let spawned = actor::spawn(
            actor::Config {
                local_session_id: id.clone(),
                agent,
                workspace_id: info.workspace.clone(),
                workspace_path: PathBuf::from(workspace.path),
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
        let startup = spawned
            .ready
            .await
            .map_err(|_| AppError::internal("ACP session actor exited during startup"))??;
        let published = self.sessions.publish(SessionEntry::new(
            info,
            spawned.handle.clone(),
            stderr_tail,
            prompt_cancel,
            startup.caps,
            startup.model_config_id,
            startup.profile_config_id,
            startup.acp_session_id,
        ))?;
        let _ = spawned.registered.send(());
        self.watch_actor_terminal(spawned.terminal, spawned.handle, published.id.clone());
        Ok(published)
    }

    /// A terminal report must match the currently registered actor generation.
    pub(super) fn watch_actor_terminal(
        &self,
        terminal: oneshot::Receiver<actor::TerminalOutcome>,
        handle: actor::Handle,
        session_id: String,
    ) {
        let sessions = self.sessions.clone();
        let permissions = Arc::clone(&self.deps.permissions);
        let event_bus = Arc::clone(&self.deps.event_bus);
        tokio::spawn(async move {
            let Ok(actor::TerminalOutcome::Failed(error)) = terminal.await else {
                return;
            };
            if !sessions.mark_failed_if_current(&session_id, handle.id()) {
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

    // Single linear rebind sequence — splitting would obscure the transfer flow.
    #[allow(clippy::too_many_lines)]
    pub(super) async fn rebind_session_inner(
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
        let (workspace_id, old_agent_id, commands) = self.sessions.rebind_start(session_id)?;
        let transfer = match super::super::conversation::export_conversation(
            &self.deps.event_bus,
            session_id,
            max_transfer_bytes,
        )
        .await
        {
            Ok(markdown) => markdown,
            Err(error) => {
                self.sessions.update_state_if(
                    session_id,
                    SessionState::Created,
                    SessionState::Idle,
                );
                return Err(error);
            }
        };
        let workspace = match self.resolve_workspace(&workspace_id).await {
            Ok(workspace) => workspace,
            Err(error) => {
                self.sessions.update_state_if(
                    session_id,
                    SessionState::Created,
                    SessionState::Idle,
                );
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
                persisted_acp_session_id: String::new(),
            },
            ACTOR_COMMAND_CAPACITY,
        );
        let startup = match spawned.ready.await {
            Ok(Ok(startup)) => startup,
            Ok(Err(error)) => {
                self.sessions.update_state(session_id, SessionState::Failed);
                return Err(error);
            }
            Err(_) => {
                self.sessions.update_state(session_id, SessionState::Failed);
                return Err(AppError::internal(
                    "ACP replacement actor exited during startup",
                ));
            }
        };
        let updated = self.sessions.rebind_finish(
            session_id,
            spawned.handle.clone(),
            stderr_tail,
            prompt_cancel,
            startup.caps,
            startup.model_config_id,
            startup.profile_config_id,
            startup.acp_session_id,
            agent_id,
            model_id,
        )?;
        let _ = spawned.registered.send(());
        self.watch_actor_terminal(spawned.terminal, spawned.handle, session_id.to_string());
        self.pipeline.reset(session_id);
        self.pipeline.queue_transfer(
            session_id.to_string(),
            super::super::conversation::ConversationTransfer {
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
        Ok(updated)
    }

    pub(super) async fn cancel_session_inner(&self, session_id: &str) -> Result<(), AppError> {
        self.ensure_live_session(session_id).await?;
        let (sender, prompt_cancel) = self.sessions.command(session_id)?;
        prompt_cancel.store(true, Ordering::Release);
        sender
            .send(ActorCommand::Cancel)
            .await
            .map_err(|_| AppError::internal("ACP session actor is unavailable"))?;
        self.sessions
            .update_state(session_id, SessionState::Interrupted);
        if let Err(error) = append_payload(
            &self.deps.event_bus,
            session_id,
            EventPayload::SessionCancelled,
        )
        .await
        {
            tracing::error!(session_id, error = %error, "failed to persist ACP session-cancelled event");
        }
        let sessions = self.sessions.clone();
        let permissions = Arc::clone(&self.deps.permissions);
        let conversation_store = self.deps.conversation_store.clone();
        let pipeline = Arc::clone(&self.pipeline);
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(CANCEL_GRACE_PERIOD).await;
            let Some(removed) = sessions.take_interrupted(&session_id) else {
                return;
            };
            let sender = removed.commands();
            let _ = sessions.remove_dormant(&session_id);
            let _ = sessions.persist(&conversation_store);
            permissions.clear_session(&session_id);
            let (closed_tx, closed_rx) = oneshot::channel();
            if sender.send(ActorCommand::Close(closed_tx)).await.is_ok() {
                let _ = closed_rx.await;
            }
            pipeline.clear(&session_id);
        });
        Ok(())
    }

    pub(super) async fn close_session_inner(&self, session_id: &str) -> Result<(), AppError> {
        if !self.has_live_session(session_id)? {
            if self.sessions.remove_dormant(session_id)?.is_none() {
                return Err(AppError::not_found_id("session", session_id));
            }
            self.deps.permissions.clear_session(session_id);
            self.pipeline.clear(session_id);
            append_payload(
                &self.deps.event_bus,
                session_id,
                EventPayload::SessionClosed,
            )
            .await
            .ok();
            return self.persist_sessions();
        }
        let entry = self.sessions.remove_live(session_id)?;
        let _ = self.sessions.remove_dormant(session_id)?;
        let persist_result = self.persist_sessions();
        self.deps.permissions.clear_session(session_id);
        let (closed_tx, closed_rx) = oneshot::channel();
        if entry
            .commands()
            .send(ActorCommand::Close(closed_tx))
            .await
            .is_ok()
        {
            let _ = closed_rx.await;
        }
        self.pipeline.clear(session_id);
        append_payload(
            &self.deps.event_bus,
            session_id,
            EventPayload::SessionClosed,
        )
        .await
        .ok();
        persist_result
    }

    pub(super) fn rename_session_inner(
        &self,
        session_id: &str,
        name: &str,
    ) -> Result<(), AppError> {
        self.sessions.rename(session_id, name)?;
        self.persist_sessions()
    }
}

#[cfg(test)]
pub(super) mod tests;
