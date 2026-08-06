//! Initial `session/set_config_option` sends at session startup.
//!
//! After `session/new` (or `session/load`), the actor sends the user-selected
//! model and profile to the agent via `session/set_config_option`. Both sends
//! are best-effort: a failure is logged but does not fail session setup, because
//! config-option selection is a hint — the agent keeps its default if the RPC
//! fails or the option is unsupported.

use agent_client_protocol::{Agent, ConnectionTo};

use super::Config;
use crate::acp::providers::{rpc_set_model_config, rpc_set_profile_config};

/// Send the initial model and profile config options to the agent.
///
/// Called after `ActorStartup` has been reported and the session is registered.
/// Each send is independent — a model send failure does not block the profile
/// send, and vice versa. Both are best-effort: the agent retains its default
/// configuration if a send fails or the option is unsupported.
///
/// # Arguments
/// * `cx` — The connection to the agent.
/// * `agent_session_id` — The ACP session id assigned by the agent.
/// * `model_config_id` — The config option id for the model category, if the
///   agent advertised one.
/// * `profile_config_id` — The config option id for the mode/profile category,
///   if the agent advertised one.
pub(super) async fn send_initial_config_options(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &agent_client_protocol::schema::v1::SessionId,
    config: &Config,
    model_config_id: Option<&str>,
    profile_config_id: Option<&str>,
) {
    send_initial_model(cx, agent_session_id, &config.local_session_id, model_config_id, &config.model_id).await;
    send_initial_profile(cx, agent_session_id, &config.local_session_id, profile_config_id, config).await;
}

/// Send the user-selected model to the agent at session startup.
///
/// Best-effort: logs a warning on failure but does not propagate the error,
/// because the agent retains its default model if the send fails.
async fn send_initial_model(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &agent_client_protocol::schema::v1::SessionId,
    local_session_id: &str,
    model_config_id: Option<&str>,
    model_id: &str,
) {
    let Some(config_id) = model_config_id else {
        tracing::info!(
            session_id = local_session_id,
            model_id = model_id,
            "agent did not advertise a model config option; skipping initial model send"
        );
        return;
    };
    if let Err(error) = rpc_set_model_config(cx, agent_session_id, config_id, model_id).await {
        tracing::warn!(
            session_id = local_session_id,
            model_id = model_id,
            error = %error,
            "initial session/set_config_option (model) failed; agent keeps its current model"
        );
    } else {
        tracing::info!(
            session_id = local_session_id,
            model_id = model_id,
            "sent initial session/set_config_option (model) on session setup"
        );
    }
}

/// Send the active profile to the agent at session startup.
///
/// Best-effort: logs a warning on failure but does not propagate the error,
/// because the agent retains its default profile if the send fails.
async fn send_initial_profile(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &agent_client_protocol::schema::v1::SessionId,
    local_session_id: &str,
    profile_config_id: Option<&str>,
    config: &Config,
) {
    let Some(config_id) = profile_config_id else {
        return;
    };
    let active_profile = config
        .profiles
        .profile(local_session_id)
        .unwrap_or_else(|_| {
            tracing::warn!(
                "profile middleware lookup failed at startup; sending default profile id"
            );
            "code".to_string()
        });
    if let Err(error) = rpc_set_profile_config(cx, agent_session_id, config_id, &active_profile).await {
        tracing::warn!(
            session_id = local_session_id,
            profile = %active_profile,
            error = %error,
            "initial session/set_config_option (profile) failed; agent keeps its current profile configuration"
        );
    } else {
        tracing::info!(
            session_id = local_session_id,
            profile = %active_profile,
            "sent initial session/set_config_option (profile) on session setup"
        );
    }
}

#[cfg(test)]
mod tests {
    // The send_initial_config_options function is a thin orchestration over
    // rpc_set_model_config and rpc_set_profile_config, which are integration-
    // tested via the contract runner. Unit testing here would require mocking
    // ConnectionTo<Agent>, which is not feasible with the current ACP crate.
    // The function's logic is simple enough (two independent best-effort sends)
    // that the risk of regression is low.
}
