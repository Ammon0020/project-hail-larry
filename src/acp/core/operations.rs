//! Prompt admission and live ACP operations.

use std::path::Path;

use chrono::Utc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
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

        // Auto-name the session from the first prompt if it still has the
        // default "New chat" name. The user can rename it later from the
        // chat history panel. Best-effort — a failure here must not block
        // the prompt.
        if let Ok(info) = self.sessions.info(session_id) {
            if info.name == "New chat" {
                let derived = derive_session_name(content);
                if let Err(error) = self.sessions.rename(session_id, &derived) {
                    tracing::warn!(%session_id, %error, "auto-name failed");
                }
            }
        }

        let (sender, caps, workspace_id, include_profile, actor_id) =
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
        // Arm the idle watchdog. It resets on any event for this session and
        // fires only if the agent goes silent for the configured timeout. The
        // token is cancelled when the prompt completes (below) so a normal
        // long-running turn does not trigger a false positive — every streamed
        // chunk/tool event keeps the timer alive, and completion cancels it.
        let watchdog_cancel = CancellationToken::new();
        self.spawn_idle_watchdog(
            session_id,
            actor_id,
            self.deps.agent_idle_timeout,
            watchdog_cancel.clone(),
        );
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
            // The watchdog stays armed while the prompt is in flight. It is
            // cancelled only after the turn completes (below) so a normal
            // long-running turn is not falsely flagged — every streamed
            // chunk/tool event keeps the timer alive, and completion cancels it.
            match result_rx.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::debug!(
                    session_id = %session_id,
                    error = %error,
                    "ACP prompt finished with error after HTTP admitted it"
                ),
                Err(_) => tracing::debug!(
                    session_id = %session_id,
                    "ACP prompt actor dropped result channel"
                ),
            }
            watchdog_cancel.cancel();
            sessions.update_state_if(&session_id, SessionState::Running, SessionState::Idle);
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

/// Derive a short session name from the first prompt content.
///
/// Takes the first ~6 words of the prompt, truncates to 60 chars, and appends
/// "…" if there's more. Newlines and excess whitespace are collapsed. The
/// result is capped to fit within `MAX_SESSION_NAME_CHARS` (128) but is
/// typically much shorter.
fn derive_session_name(content: &str) -> String {
    const MAX_WORDS: usize = 6;
    const MAX_CHARS: usize = 60;

    let cleaned: String = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    let words: Vec<&str> = cleaned.split_whitespace().take(MAX_WORDS).collect();
    let mut title = words.join(" ");

    // If there were more words, append ellipsis.
    let total_words = cleaned.split_whitespace().count();
    if total_words > MAX_WORDS {
        title.push('…');
    }

    // Truncate by character count if still too long.
    if title.chars().count() > MAX_CHARS {
        let truncated: String = title.chars().take(MAX_CHARS - 1).collect();
        title = format!("{truncated}…");
    }

    if title.is_empty() {
        "New chat".to_string()
    } else {
        title
    }
}

#[cfg(test)]
mod tests {
    use super::derive_session_name;

    #[test]
    fn derive_from_short_prompt() {
        assert_eq!(derive_session_name("Fix the auth bug"), "Fix the auth bug");
    }

    #[test]
    fn derive_from_long_prompt_truncates_words() {
        let prompt = "Please help me refactor the authentication module to use async await patterns throughout the codebase";
        let name = derive_session_name(prompt);
        assert!(name.ends_with('…'));
        assert!(name.chars().count() <= 61); // 60 chars + ellipsis
    }

    #[test]
    fn derive_collapses_newlines_and_whitespace() {
        let prompt = "Fix the\n\n  auth  bug\n  in the router";
        assert_eq!(derive_session_name(prompt), "Fix the auth bug in the…");
    }

    #[test]
    fn derive_empty_prompt_falls_back() {
        assert_eq!(derive_session_name(""), "New chat");
        assert_eq!(derive_session_name("   \n  \n  "), "New chat");
    }

    #[test]
    fn derive_truncates_by_chars_when_words_are_long() {
        let prompt = "supercalifragilisticexpialidocious_is_a_very_long_word_that_exceeds_the_character_limit_easily";
        let name = derive_session_name(prompt);
        assert!(name.ends_with('…'));
        assert!(name.chars().count() <= 61);
    }
}
