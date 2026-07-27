//! Prompt admission and live ACP operations.

use std::path::Path;

use chrono::Utc;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::actor::ActorCommand;
use super::events::append_payload;
use super::registry::SessionState;
use super::{Client, MODEL_SWITCH_TRANSFER_BYTES};
use crate::interfaces::{AppError, Attachment, EventPayload, ProviderInfo, SessionInfo};

impl Client {
    #[cfg(test)]
    pub(super) fn session_for_profile_switch(
        &self,
        session_id: &str,
    ) -> Result<(tokio::sync::mpsc::Sender<ActorCommand>, Option<String>), AppError> {
        self.sessions.profile_command(session_id)
    }

    pub(super) async fn create_session_with_profile_inner(
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
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "skipping profile MCP server-name validation because mcp config is unavailable");
                }
            }
        }
        self.deps
            .registry
            .resolve(agent_id, model_id)
            .map_err(AppError::validation)?;
        self.resolve_workspace(workspace_id).await?;
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
            tracing::error!(session_id = %published.id, error = %error, "failed to persist new ACP session");
            let _ = self.close_session_inner(&published.id).await;
            return Err(error);
        }
        append_payload(
            &self.deps.event_bus,
            &published.id,
            EventPayload::SessionCreated,
        )
        .await?;
        Ok(published)
    }

    pub(super) async fn send_prompt_inner(
        &self,
        session_id: &str,
        content: &str,
        attachments: &[Attachment],
    ) -> Result<(), AppError> {
        self.ensure_live_session(session_id).await?;
        let (sender, caps, workspace_id, include_profile) =
            self.sessions.begin_prompt(session_id)?;
        let workspace = self.resolve_workspace(&workspace_id).await?;
        let prepared = match self
            .pipeline
            .prepare(
                session_id,
                &workspace_id,
                Path::new(&workspace.path),
                caps.embedded_context,
                include_profile,
                self.deps.workspaces.as_ref(),
            )
            .await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                self.sessions.update_state_if(
                    session_id,
                    SessionState::Running,
                    SessionState::Idle,
                );
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
            self.sessions.update_state(session_id, SessionState::Failed);
            append_payload(&self.deps.event_bus, session_id, EventPayload::AgentExited { content: "ACP session actor is unavailable".to_string() }).await
                .map_err(|error| { tracing::error!(session_id, error = %error, "failed to persist ACP prompt-dispatch failure"); error })?;
            return Err(AppError::internal("ACP session actor is unavailable"));
        }
        // Return as soon as the turn is admitted. Holding the HTTP request open
        // for the full agent turn pinches the browser's ~6 connections/origin
        // when multiple tabs/sessions run at once, which surfaces as gateway
        // timeouts on unrelated API calls while streams continue over WS.
        // Completion, errors, and cancel still publish through the event bus;
        // a background watcher restores Idle only from Running so an explicit
        // cancel → Interrupted transition is not clobbered.
        let sessions = self.sessions.clone();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            match result_rx.await {
                Ok(Ok(())) => {
                    sessions.update_state_if(
                        &session_id,
                        SessionState::Running,
                        SessionState::Idle,
                    );
                }
                Ok(Err(error)) => {
                    tracing::debug!(
                        session_id = %session_id,
                        error = %error,
                        "ACP prompt finished with error after HTTP admitted it"
                    );
                    sessions.update_state_if(
                        &session_id,
                        SessionState::Running,
                        SessionState::Idle,
                    );
                }
                Err(_) => {
                    tracing::debug!(
                        session_id = %session_id,
                        "ACP prompt actor dropped result channel"
                    );
                    sessions.update_state_if(
                        &session_id,
                        SessionState::Running,
                        SessionState::Idle,
                    );
                }
            }
        });
        Ok(())
    }

    pub(super) async fn switch_model_inner(
        &self,
        session_id: &str,
        model_id: &str,
    ) -> Result<(), AppError> {
        self.ensure_live_session(session_id).await?;
        let (sender, model_config_id) = self.sessions.model_command(session_id)?;
        let Some(config_id) = model_config_id else {
            let current = self.sessions.info(session_id)?;
            if current.status != SessionState::Idle.as_str() {
                return Err(AppError::unsupported(format!(
                    "{}; session must be idle for rebind fallback",
                    super::super::providers::MODEL_SWITCH_UNSUPPORTED_MSG
                )));
            }
            return self
                .rebind_session_inner(
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
        self.sessions.update_model(session_id, model_id)?;
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

    pub(super) async fn set_session_profile_inner(
        &self,
        session_id: &str,
        profile: &str,
    ) -> Result<(), AppError> {
        let config = self.pipeline.profiles.config()?;
        let trimmed = profile.trim();
        if trimmed.is_empty() {
            return Err(AppError::validation("profile id is required"));
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
        let normalized = config.normalize_profile_id(profile);
        if !self.has_live_session(session_id)? && !self.sessions.contains_dormant(session_id)? {
            return Err(AppError::not_found_id("session", session_id));
        }
        match self.sessions.profile_command(session_id) {
            Ok((sender, Some(config_id))) => {
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
            }
            Ok((_, None)) => {}
            Err(error) => {
                tracing::debug!(session_id, profile = %normalized, error = %error, "set_session_profile: session not live; profile stored for next prompt");
            }
        }
        self.pipeline.profiles.set_profile(session_id, profile)
    }

    pub(super) async fn list_providers_inner(
        &self,
        session_id: &str,
    ) -> Result<Vec<ProviderInfo>, AppError> {
        self.ensure_live_session(session_id).await?;
        let (sender, caps) = self.sessions.provider_command(session_id)?;
        super::super::providers::require_providers_supported(caps)?;
        let (result_tx, result_rx) = oneshot::channel();
        sender
            .send(ActorCommand::ListProviders { result: result_tx })
            .await
            .map_err(|_| AppError::internal("ACP session actor is unavailable"))?;
        result_rx
            .await
            .map_err(|_| AppError::internal("ACP list_providers actor exited"))?
    }

    pub(super) async fn set_provider_inner(
        &self,
        session_id: &str,
        id: &str,
        api_type: &str,
        base_url: &str,
        headers: std::collections::HashMap<String, String>,
    ) -> Result<(), AppError> {
        self.ensure_live_session(session_id).await?;
        let (sender, caps) = self.sessions.provider_command(session_id)?;
        super::super::providers::require_providers_supported(caps)?;
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

    pub(super) async fn disable_provider_inner(
        &self,
        session_id: &str,
        id: &str,
    ) -> Result<(), AppError> {
        self.ensure_live_session(session_id).await?;
        let (sender, caps) = self.sessions.provider_command(session_id)?;
        super::super::providers::require_providers_supported(caps)?;
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
