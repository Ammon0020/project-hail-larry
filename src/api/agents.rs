//! Agent registration, removal, and autodetection handlers.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};

use crate::acp;
use crate::config::AgentInfo;
use crate::interfaces::ACPClient;
use crate::sync::is_loopback_addr;

use super::auth::PeerAddr;
use super::{app_error, decode_json_body, ApiResponseError, AppState};

/// Cached autodetect results: `(timestamp, agent list)`.
pub(super) type AutodetectCache = Arc<Mutex<Option<(Instant, Vec<AgentInfo>)>>>;

/// Per-field size limits for agent config (defense-in-depth on top of loopback gate).
const MAX_AGENT_COMMAND_LEN: usize = 1024;
const MAX_AGENT_ARGS_COUNT: usize = 64;
const MAX_AGENT_MODELS_COUNT: usize = 256;

/// Cooldown for autodetect probe spawning to prevent resource exhaustion from
/// repeated calls. Each autodetect spawns up to 5 child processes.
const AUTODETECT_COOLDOWN: Duration = Duration::from_secs(60);

pub(super) async fn list_agents(
    State(state): State<AppState>,
) -> Result<Json<Vec<AgentInfo>>, ApiResponseError> {
    state.acp.list_agents().await.map(Json).map_err(app_error)
}

pub(super) async fn upsert_agent(
    State(state): State<AppState>,
    PeerAddr(remote_addr): PeerAddr,
    body: Result<Json<AgentInfo>, JsonRejection>,
) -> Result<Json<AgentInfo>, ApiResponseError> {
    // Agent registration persists an arbitrary command that is spawned as a
    // child process, so restrict it to loopback callers to avoid an RCE
    // vector from paired LAN devices.
    if !is_loopback_addr(&remote_addr) {
        return Err(ApiResponseError::forbidden(
            "Agent registration is only allowed from loopback. Use 'app add-agent' on the host.",
        ));
    }
    let Json(agent) = decode_json_body(body)?;
    if agent.id.trim().is_empty() || agent.command.trim().is_empty() {
        return Err(ApiResponseError::bad_request(
            "agent id and command are required",
        ));
    }
    // Per-field size limits: reject oversized agent configs before persisting
    // (defense-in-depth on top of the loopback gate).
    if agent.command.len() > MAX_AGENT_COMMAND_LEN {
        return Err(ApiResponseError::bad_request(format!(
            "agent command exceeds {MAX_AGENT_COMMAND_LEN} characters"
        )));
    }
    if agent.args.len() > MAX_AGENT_ARGS_COUNT {
        return Err(ApiResponseError::bad_request(format!(
            "too many agent args (max {MAX_AGENT_ARGS_COUNT})"
        )));
    }
    if agent.models.len() > MAX_AGENT_MODELS_COUNT {
        return Err(ApiResponseError::bad_request(format!(
            "too many agent models (max {MAX_AGENT_MODELS_COUNT})"
        )));
    }
    state.acp.register_agent(agent.clone());
    let mut config = state.config.write();
    config.upsert_agent(agent.clone()).map_err(|error| {
        ApiResponseError::internal(format!("persist agent configuration: {error}"))
    })?;
    Ok(Json(agent))
}

pub(super) async fn delete_agent(
    State(state): State<AppState>,
    PeerAddr(remote_addr): PeerAddr,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiResponseError> {
    // Agent deletion mutates persisted config and the live agent registry;
    // restrict it to loopback callers, matching `upsert_agent`.
    if !is_loopback_addr(&remote_addr) {
        return Err(ApiResponseError::forbidden(
            "Agent deletion is only allowed from loopback. Use 'app remove-agent <id>' on the host.",
        ));
    }
    state.acp.remove_agent(&id);
    state.config.write().delete_agent(&id).map_err(|error| {
        ApiResponseError::internal(format!("persist agent configuration: {error}"))
    })?;
    Ok(Json(json!({"status": "deleted"})))
}

pub(super) async fn autodetect_agents(State(state): State<AppState>) -> Json<Vec<AgentInfo>> {
    // Rate-limit probe spawning: return cached results if within the cooldown
    // so repeated calls don't re-spawn child processes (DoS defense).
    if let Ok(cache) = state.autodetect_cache.lock() {
        if let Some((at, ref results)) = *cache {
            if at.elapsed() < AUTODETECT_COOLDOWN {
                return Json(results.clone());
            }
        }
    }
    let results = acp::autodetect().await;
    if let Ok(mut cache) = state.autodetect_cache.lock() {
        *cache = Some((Instant::now(), results.clone()));
    }
    Json(results)
}
