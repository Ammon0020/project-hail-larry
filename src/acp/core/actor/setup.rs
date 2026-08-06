//! Best-effort initial `session/set_config_option` sends.
//!
//! Unsupported values must not fail session setup; the agent keeps its current
//! model or profile when either independent RPC fails.

use agent_client_protocol::{Agent, ConnectionTo};

use super::Config;
use crate::acp::providers::{rpc_set_model_config, rpc_set_profile_config};

pub(super) async fn send_initial_config_options(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &agent_client_protocol::schema::v1::SessionId,
    config: &Config,
    model_config_id: Option<&str>,
    profile_config_id: Option<&str>,
) {
    if let Some(config_id) = model_config_id {
        let result = rpc_set_model_config(cx, agent_session_id, config_id, &config.model_id).await;
        log_result(result, &config.local_session_id, "model", &config.model_id);
    } else {
        tracing::info!(
            session_id = config.local_session_id,
            model_id = config.model_id,
            "agent did not advertise a model config option; skipping initial model send"
        );
    }

    let Some(config_id) = profile_config_id else {
        return;
    };
    let profile = config
        .profiles
        .profile(&config.local_session_id)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "profile lookup failed; using default profile");
            "code".to_string()
        });
    let result = rpc_set_profile_config(cx, agent_session_id, config_id, &profile).await;
    log_result(result, &config.local_session_id, "profile", &profile);
}

fn log_result(
    result: Result<(), crate::interfaces::AppError>,
    session_id: &str,
    kind: &str,
    value: &str,
) {
    match result {
        Ok(()) => tracing::info!(
            session_id,
            kind,
            value,
            "sent initial session/set_config_option"
        ),
        Err(error) => tracing::warn!(
            session_id,
            kind,
            value,
            %error,
            "initial session/set_config_option failed; agent keeps its current configuration"
        ),
    }
}
