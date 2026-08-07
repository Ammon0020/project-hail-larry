//! Profile changes that ACP cannot apply to a running agent session.
//!
//! A session's MCP server list is fixed at `session/new`. When a profile's
//! server allowlist differs from the one the live session started with, the
//! only honest options are to move the conversation to a new agent session or
//! to open a separate one — never to claim in place that tool access changed.
//! This module implements both, plus the read-only preview the UI uses to
//! decide whether it needs to ask at all.

use agent_client_protocol::schema::v1::McpCapabilities;

use super::super::profile_config::ProfileConfig;
use super::Client;
use crate::interfaces::{
    AppError, ProfileTransitionPreview, ProfileTransitionStrategy, SessionInfo,
};

/// Validate a client-supplied profile id and return its canonical form.
///
/// Shared by session creation, the in-place profile set, and transitions so all
/// three accept and reject exactly the same inputs. Matching is
/// case-insensitive; unknown ids are rejected rather than silently falling back
/// to the default profile, because a silent fallback would give the user a
/// different profile than the one they picked.
///
/// # Errors
///
/// Returns a validation error for a blank id or one absent from the config.
pub(super) fn validated_profile_id(
    config: &ProfileConfig,
    profile: &str,
) -> Result<String, AppError> {
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
    Ok(config.normalize_profile_id(trimmed))
}

impl Client {
    pub(super) fn preview_session_profile_inner(
        &self,
        session_id: &str,
        profile: &str,
    ) -> Result<ProfileTransitionPreview, AppError> {
        let config = self.pipeline.profiles.config()?;
        let target = validated_profile_id(&config, profile)?;
        // Errors for an unknown session; `None` for a known dormant one.
        let caps = self.sessions.mcp_transport_caps(session_id)?;
        let current = self.pipeline.profiles.profile(session_id)?;

        let Some(file) = self.load_mcp_file_for_preview() else {
            // No MCP configuration means no server access either way, so no
            // profile switch can change it.
            return Ok(ProfileTransitionPreview {
                requires_new_session: false,
                current_servers: Vec::new(),
                target_servers: Vec::new(),
            });
        };
        // A dormant session has never run `initialize`, so its agent's
        // transports are unknown. Assume stdio only — the conservative floor
        // every agent supports.
        let mcp_caps = caps.map_or_else(McpCapabilities::new, |caps| {
            McpCapabilities::new().http(caps.mcp_http).sse(caps.mcp_sse)
        });
        let current_servers = super::mcp::effective_server_names(
            &file,
            &mcp_caps,
            self.pipeline
                .profiles
                .mcp_servers_for_profile(&current)?
                .as_deref(),
        );
        let target_servers = super::mcp::effective_server_names(
            &file,
            &mcp_caps,
            self.pipeline
                .profiles
                .mcp_servers_for_profile(&target)?
                .as_deref(),
        );
        Ok(ProfileTransitionPreview {
            // Both sides come from one config walk, so ordering already matches
            // and a plain comparison is a set comparison.
            requires_new_session: current_servers != target_servers,
            current_servers,
            target_servers,
        })
    }

    pub(super) async fn transition_session_profile_inner(
        &self,
        session_id: &str,
        profile: &str,
        strategy: ProfileTransitionStrategy,
        max_transfer_bytes: i64,
    ) -> Result<SessionInfo, AppError> {
        let config = self.pipeline.profiles.config()?;
        let target = validated_profile_id(&config, profile)?;
        // Resolve before any teardown so a bad request changes nothing. Works
        // for dormant sessions too, which matters for `Fresh`.
        let current = self.sessions.info(session_id)?;

        match strategy {
            ProfileTransitionStrategy::Fresh => {
                self.create_session_with_profile_inner(
                    &current.agent_id,
                    &current.model_id,
                    &current.workspace,
                    Some(&target),
                )
                .await
            }
            ProfileTransitionStrategy::History => {
                self.transition_history(session_id, &current, &target, max_transfer_bytes)
                    .await
            }
        }
    }

    /// Rebind this conversation to a replacement agent session under `target`.
    ///
    /// The profile must be stored *before* the replacement actor spawns: the
    /// actor reads the active profile's allowlist when it builds `session/new`,
    /// so a later write would apply the instructions without the servers.
    async fn transition_history(
        &self,
        session_id: &str,
        current: &SessionInfo,
        target: &str,
        max_transfer_bytes: i64,
    ) -> Result<SessionInfo, AppError> {
        let previous = self.pipeline.profiles.profile(session_id)?;
        self.pipeline.profiles.set_profile(session_id, target)?;
        let label = self
            .pipeline
            .profiles
            .config()?
            .profile(target)
            .label
            .clone();
        // An unspecified budget is floored by the rebind path itself.
        let result = self
            .rebind_session_with_notice(
                session_id,
                &current.agent_id,
                &current.model_id,
                max_transfer_bytes,
                &format!("Started a new agent session with the {label} profile."),
            )
            .await;
        if result.is_err() {
            // The replacement session never reached `session/new`, so the target
            // profile's server list was never applied. Roll the stored selection
            // back so the UI keeps showing the access the user actually has.
            if let Err(error) = self.pipeline.profiles.set_profile(session_id, &previous) {
                tracing::error!(
                    session_id,
                    %error,
                    "failed to restore the previous profile after a failed transition"
                );
            }
        }
        result
    }

    /// Load `mcp.json` for a preview, or `None` when it is absent/unreadable.
    ///
    /// Preview is advisory: an unreadable config must not fail the request, and
    /// reporting "no servers" matches what session setup would actually attach.
    fn load_mcp_file_for_preview(&self) -> Option<crate::mcp::File> {
        let path = self.deps.mcp_config_path.as_deref()?;
        match crate::mcp::File::load(path) {
            Ok(file) => Some(file),
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    %error,
                    "profile preview: mcp config unavailable; reporting no server access"
                );
                None
            }
        }
    }
}
