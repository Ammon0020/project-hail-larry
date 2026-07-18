//! HTTP API composition and first UI-smoke handlers.
//!
//! The router deliberately keeps security policy at the edge: pairing is the
//! only unauthenticated API, loopback requests bypass device credentials with
//! an Origin check on mutations, and the WebSocket hub performs its own
//! browser-specific credential and Origin gate.

mod embed;
mod mcp;
mod providers;
mod session_extra;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, FromRequestParts, OriginalUri, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, error, warn};

use crate::acp::{self, Client};
use crate::config::{AgentInfo, ConfigStore};
use crate::events::SharedEventBus;
use crate::interfaces::{
    map_api_error, ACPClient, AppError, Attachment, Event, EventStore, EventType,
    PermissionDecision, PermissionManager, SearchOptions, WorkspaceManager,
};
use crate::pairing::{Manager as PairingManager, PairingError};
use crate::permissions::Manager as PermissionsManager;
use crate::sync::{is_loopback_addr, AuthChecker, Hub};
use crate::uploads;
use crate::workspace::Manager as WorkspaceManagerImpl;

/// JSON body size limit for API requests other than file writes and uploads.
/// Caps malformed or abusive LAN requests before parsing (Go `defaultMaxBodyBytes`).
pub const MAX_API_BODY_BYTES: usize = 10 * 1024 * 1024;

/// File-write exception matching Go `fileWriteMaxBodyBytes` (large editor saves).
pub const FILE_WRITE_MAX_BODY_BYTES: usize = 50 * 1024 * 1024;
const MAX_EVENT_LIMIT: i32 = 10_000;
const DEFAULT_EVENT_LIMIT: i32 = 1_000;
const PAIR_RATE_PER_MINUTE: f64 = 5.0;
const PAIR_RATE_BURST: f64 = 5.0;

/// Concrete services shared by handlers. Construction is owned by
/// [`crate::app::listen`], keeping HTTP handlers free of global state.
#[derive(Clone)]
pub struct AppState {
    pub config: ConfigStore,
    pub pairing: PairingManager,
    pub workspaces: Arc<WorkspaceManagerImpl>,
    pub events: SharedEventBus,
    pub hub: Arc<Hub>,
    pub acp: Arc<Client>,
    pub permissions: Arc<PermissionsManager>,
    /// Absolute path to `mcp.json`. `None` makes `/api/mcp*` return 503.
    pub mcp_config_path: Option<PathBuf>,
    /// Per-session image store. `None` makes upload routes return 503.
    pub uploads: Option<Arc<Mutex<uploads::Manager>>>,
    /// External filesystem watcher. `None` when notify init failed.
    pub fs_watcher: Option<Arc<crate::fswatch::Watcher>>,
    pair_rate: Arc<Mutex<HashMap<String, PairRateBucket>>>,
}

/// Per-IP token bucket matching Go's five-request burst and 5/minute refill.
struct PairRateBucket {
    tokens: f64,
    updated_at: Instant,
}

impl AppState {
    /// Build state from already-composed service instances.
    ///
    /// The daemon composition root supplies all concrete services explicitly;
    /// a parameter object would only duplicate this state struct at the API
    /// boundary, so the intentionally wide constructor remains direct.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: ConfigStore,
        pairing: PairingManager,
        workspaces: Arc<WorkspaceManagerImpl>,
        events: SharedEventBus,
        hub: Arc<Hub>,
        acp: Arc<Client>,
        permissions: Arc<PermissionsManager>,
        mcp_config_path: Option<PathBuf>,
        uploads: Option<Arc<Mutex<uploads::Manager>>>,
        fs_watcher: Option<Arc<crate::fswatch::Watcher>>,
    ) -> Self {
        Self {
            config,
            pairing,
            workspaces,
            events,
            hub,
            acp,
            permissions,
            mcp_config_path,
            uploads,
            fs_watcher,
            pair_rate: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// Construct the UI-smoke router.
///
/// Callers serving TCP must use Axum's `into_make_service_with_connect_info` so
/// the loopback authorization decision uses the real peer address.
pub fn router(state: AppState) -> Router {
    let auth_state = state.clone();
    let protected: Router = Router::new()
        .route("/api/devices", get(list_devices))
        .route("/api/devices/{id}", delete(revoke_device))
        .route("/api/devices/cancel-revocation", post(cancel_revocation))
        .route("/api/pending-actions", get(list_pending_actions))
        .route(
            "/api/workspaces",
            get(list_workspaces).post(register_workspace),
        )
        .route("/api/workspaces/{id}", delete(remove_workspace))
        .route(
            "/api/workspaces/cancel-registration",
            post(cancel_workspace_registration),
        )
        .route("/api/workspaces/{id}/files", get(file_tree))
        // POST write lives on a sibling router with a 50 MiB body cap (below).
        .route("/api/workspaces/{id}/file", get(read_file))
        .route("/api/workspaces/{id}/raw", get(raw_file))
        .route("/api/workspaces/{id}/search", get(search))
        .route("/api/events", get(events))
        .route("/api/events/{session_id}", get(session_events))
        .route("/api/agents", get(list_agents).post(upsert_agent))
        .route("/api/agents/{id}", delete(delete_agent))
        .route("/api/agents/autodetect", post(autodetect_agents))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/{id}",
            get(get_session).patch(patch_session).delete(close_session),
        )
        .route("/api/sessions/{id}/prompt", post(send_prompt))
        .route("/api/sessions/{id}/cancel", post(cancel_session))
        .route(
            "/api/sessions/{id}/export",
            get(session_extra::export_session),
        )
        .route(
            "/api/sessions/{id}/context",
            post(session_extra::session_context),
        )
        .route(
            "/api/sessions/{id}/uploads",
            post(session_extra::upload_file),
        )
        .route(
            "/api/sessions/{id}/uploads/{upload_id}",
            get(session_extra::serve_upload),
        )
        .route(
            "/api/sessions/{id}/providers",
            get(providers::list_providers),
        )
        .route(
            "/api/sessions/{id}/providers/{provider_id}",
            put(providers::set_provider).delete(providers::disable_provider),
        )
        .route("/api/mcp", get(mcp::get_mcp).put(mcp::put_mcp))
        .route("/api/mcp/servers/{name}", patch(mcp::patch_mcp_server))
        .route("/api/mcp/status", get(mcp::get_mcp_status))
        .route("/api/permissions/pending", get(pending_permissions))
        .route("/api/permissions/{id}/respond", post(respond_permission))
        .route_layer(middleware::from_fn_with_state(
            auth_state.clone(),
            require_auth,
        ))
        .layer(DefaultBodyLimit::max(MAX_API_BODY_BYTES))
        .with_state(state.clone());

    // Large editor saves need Go's 50 MiB write exception; a global 10 MiB
    // DefaultBodyLimit cannot be raised per-route once applied as an outer
    // layer, so write POST is a sibling authenticated router.
    let file_write: Router = Router::new()
        .route("/api/workspaces/{id}/file", post(write_file))
        .route_layer(middleware::from_fn_with_state(auth_state, require_auth))
        .layer(DefaultBodyLimit::max(FILE_WRITE_MAX_BODY_BYTES))
        .with_state(state.clone());

    // The hub owns its own handshake authorization because WebSocket
    // credentials are browser query parameters rather than Authorization.
    state
        .hub
        .set_auth_checker(pairing_auth_checker(&state.pairing));

    let pairing_public: Router = Router::new()
        .route("/api/pair/initiate", post(pair_initiate))
        .route("/api/pair/verify-passcode", post(pair_verify_passcode))
        .route("/api/pair/verify-token", post(pair_verify_token))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_pair_rate_limit,
        ))
        .layer(DefaultBodyLimit::max(MAX_API_BODY_BYTES))
        .with_state(state.clone());
    let public: Router = Router::new()
        .route("/health", get(health))
        .merge(pairing_public);

    Router::new()
        .merge(public)
        .merge(protected)
        .merge(file_write)
        .merge(state.hub.clone().into_router())
        .fallback(get(spa_fallback))
}

fn pairing_auth_checker(manager: &PairingManager) -> AuthChecker {
    let manager = manager.clone();
    Arc::new(move |device_id, secret| manager.validate_credential(device_id, secret))
}

/// `GET /health` is intentionally unauthenticated for host/service probes.
async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PairInitiateRequest {
    host: Option<String>,
    port: Option<u16>,
}

async fn pair_initiate(
    State(state): State<AppState>,
    body: Result<Json<PairInitiateRequest>, JsonRejection>,
) -> Result<Json<crate::interfaces::PairingSession>, ApiResponseError> {
    // Empty/missing JSON is allowed (defaults); syntax errors are 400.
    let request = match body {
        Ok(Json(request)) => request,
        Err(rejection) => {
            if matches!(
                rejection,
                JsonRejection::MissingJsonContentType(_) | JsonRejection::BytesRejection(_)
            ) {
                PairInitiateRequest {
                    host: None,
                    port: None,
                }
            } else {
                return Err(ApiResponseError::bad_request("invalid request body"));
            }
        }
    };
    let configured = state.config.read().clone();
    let mut host = request
        .host
        .filter(|host| !host.is_empty())
        .unwrap_or(configured.host);
    if host == "0.0.0.0" || host == "::" {
        host = "localhost".to_string();
    }
    let port = request
        .port
        .unwrap_or_else(|| u16::try_from(configured.port).unwrap_or(7337));
    state
        .pairing
        .create_session(&host, port)
        .map(Json)
        .map_err(pairing_error)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairVerifyRequest {
    passcode: Option<String>,
    token: Option<String>,
    device_name: String,
}

async fn pair_verify_passcode(
    State(state): State<AppState>,
    body: Result<Json<PairVerifyRequest>, JsonRejection>,
) -> Result<Json<crate::interfaces::DeviceCredential>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    state
        .pairing
        .verify_passcode(
            request.passcode.as_deref().unwrap_or_default(),
            request.device_name,
        )
        .map(Json)
        .map_err(pairing_error)
}

async fn pair_verify_token(
    State(state): State<AppState>,
    body: Result<Json<PairVerifyRequest>, JsonRejection>,
) -> Result<Json<crate::interfaces::DeviceCredential>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    state
        .pairing
        .verify_token(
            request.token.as_deref().unwrap_or_default(),
            request.device_name,
        )
        .map(Json)
        .map_err(pairing_error)
}

async fn list_devices(State(state): State<AppState>) -> Json<Vec<crate::interfaces::DeviceInfo>> {
    Json(state.pairing.list_devices())
}

/// `DELETE /api/devices/{id}` — immediate revoke (200) or grace-period pending (202).
async fn revoke_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Response, ApiResponseError> {
    let grace = revocation_grace_period(&state);
    let requester = device_id_from_request(&headers, uri.query());
    let info = state
        .pairing
        .request_revocation(&id, requester, grace)
        .map_err(pairing_error)?;

    if grace.is_zero() {
        return Ok((StatusCode::OK, Json(json!({"status": "revoked"}))).into_response());
    }

    // Broadcast so every connected device can surface and cancel the action.
    let mut event = Event::new(0, EventType::DeviceRevocationPending, "", Utc::now());
    event.target = info.device_id.clone();
    event.device_name = info.device_name.clone();
    event.request_id = info.id.clone();
    event.command = info.requested_by.clone();
    event.execute_at = info.execute_at;
    record_event(&state, event).await;

    Ok((StatusCode::ACCEPTED, Json(info)).into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CancelActionRequest {
    action_id: String,
}

/// `POST /api/devices/cancel-revocation` — body `{"actionId":"..."}`.
async fn cancel_revocation(
    State(state): State<AppState>,
    body: Result<Json<CancelActionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    cancel_pending_action(
        &state,
        body,
        |id| state.pairing.cancel_revocation(id),
        EventType::DeviceRevocationCancelled,
    )
    .await
}

/// `GET /api/pending-actions` — all grace-period pending actions.
async fn list_pending_actions(
    State(state): State<AppState>,
) -> Json<Vec<crate::interfaces::PendingActionInfo>> {
    Json(state.pairing.list_pending_actions())
}

async fn list_workspaces(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::interfaces::WorkspaceInfo>>, ApiResponseError> {
    state.workspaces.list().await.map(Json).map_err(app_error)
}

#[derive(Deserialize)]
struct RegisterWorkspaceRequest {
    path: String,
}

/// `POST /api/workspaces` — remote devices need the config gate; the host CLI
/// (loopback) can always register immediately so `add-folder` updates the live
/// daemon without a restart.
async fn register_workspace(
    State(state): State<AppState>,
    PeerAddr(remote_addr): PeerAddr,
    headers: HeaderMap,
    uri: Uri,
    body: Result<Json<RegisterWorkspaceRequest>, JsonRejection>,
) -> Result<Response, ApiResponseError> {
    let Json(payload) = decode_json_body(body)?;
    let loopback = is_loopback_addr(&remote_addr);
    let allow_remote = state.config.read().allow_remote_workspace_registration;
    if !allow_remote && !loopback {
        return Err(ApiResponseError::forbidden(
            "Remote workspace registration is disabled. Use 'app add-folder <path>' on the host, or set allowRemoteWorkspaceRegistration: true in config.",
        ));
    }

    let grace = revocation_grace_period(&state);
    // Host CLI and zero-grace paths register immediately into the live manager.
    if loopback || grace.is_zero() {
        let workspace = state
            .workspaces
            .register(&payload.path)
            .await
            .map_err(app_error)?;
        if let Some(watcher) = state.fs_watcher.as_ref() {
            watcher.add_workspace(&workspace.id, &workspace.path);
        }
        // Keep config.toml aligned when the host CLI (or zero-grace remote)
        // registers into a running daemon.
        if let Err(error) = state.config.write().add_workspace(&workspace.path) {
            warn!(
                path = %workspace.path,
                %error,
                "registered workspace but failed to persist config"
            );
        }
        return Ok((StatusCode::CREATED, Json(workspace)).into_response());
    }

    let requester = device_id_from_request(&headers, uri.query());
    let info = state
        .pairing
        .request_workspace_registration(&payload.path, requester, grace)
        .map_err(pairing_error)?;

    let mut event = Event::new(0, EventType::WorkspaceRegistrationPending, "", Utc::now());
    event.target = info.path.clone();
    event.request_id = info.id.clone();
    event.command = info.requested_by.clone();
    event.execute_at = info.execute_at;
    record_event(&state, event).await;

    Ok((StatusCode::ACCEPTED, Json(info)).into_response())
}

/// `DELETE /api/workspaces/{id}` — host/loopback unregistration for
/// `remove-folder` against a running daemon. LAN clients stay on the remote
/// registration gate and cannot delete workspaces here.
async fn remove_workspace(
    State(state): State<AppState>,
    PeerAddr(remote_addr): PeerAddr,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiResponseError> {
    if !is_loopback_addr(&remote_addr) {
        return Err(ApiResponseError::forbidden(
            "Workspace removal from the network is disabled. Use 'app remove-folder <id>' on the host.",
        ));
    }
    let workspaces = state.workspaces.list().await.map_err(app_error)?;
    let workspace = workspaces
        .into_iter()
        .find(|workspace| workspace.id == id)
        .ok_or_else(|| ApiResponseError::not_found(format!("workspace {id} not found")))?;
    state.workspaces.remove(&id).await.map_err(app_error)?;
    if let Some(watcher) = state.fs_watcher.as_ref() {
        watcher.remove_workspace(&id);
    }
    if let Err(error) = state.config.write().remove_workspace(&workspace.path) {
        // Already removed from the live manager; config drift is loud but not
        // fatal for the CLI operator who may have already saved config.
        warn!(
            path = %workspace.path,
            %error,
            "removed live workspace but config persist failed"
        );
    }
    Ok(Json(json!({ "status": "removed", "id": id })))
}

/// Peer address for loopback checks. Missing `ConnectInfo` (unit tests) is
/// treated as loopback, matching `require_auth`.
struct PeerAddr(String);

impl<S> FromRequestParts<S> for PeerAddr
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(peer_addr_string(&parts.extensions)))
    }
}

/// `POST /api/workspaces/cancel-registration` — body `{"actionId":"..."}`.
async fn cancel_workspace_registration(
    State(state): State<AppState>,
    body: Result<Json<CancelActionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    cancel_pending_action(
        &state,
        body,
        |id| state.pairing.cancel_workspace_registration(id),
        EventType::WorkspaceRegistrationCancelled,
    )
    .await
}

async fn file_tree(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<crate::interfaces::FileNode>>, ApiResponseError> {
    state
        .workspaces
        .file_tree(&id)
        .await
        .map(Json)
        .map_err(app_error)
}

#[derive(Deserialize)]
struct FileQuery {
    path: Option<String>,
}

async fn read_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Json<Value>, ApiResponseError> {
    let path = required_query(query.path, "path")?;
    let file = state
        .workspaces
        .read_file(&id, &path)
        .await
        .map_err(app_error)?;
    Ok(Json(json!({
        "content": file.content,
        "revision": file.revision,
        "path": path,
        "isBinary": file.is_binary,
        "previewable": file.previewable,
    })))
}

async fn raw_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Response, ApiResponseError> {
    let path = required_query(query.path, "path")?;
    let absolute = state
        .workspaces
        .file_path(&id, &path)
        .await
        .map_err(app_error)?;
    let data = tokio::fs::read(&absolute)
        .await
        .map_err(|error| ApiResponseError::internal(format!("read raw file: {error}")))?;
    // Go http.ServeFile uses mime.TypeByExtension then sniffing. Prefer the
    // extension so README.md is text/markdown; charset=utf-8.
    let content_type = content_type_for_path(std::path::Path::new(&absolute), &data);
    let mut response = Response::new(Body::from(data));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
    let content_type = HeaderValue::try_from(content_type).map_err(|error| {
        ApiResponseError::internal(format!("derive raw file content type: {error}"))
    })?;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    Ok(response)
}

/// MIME type for raw file serving — extension first, then magic sniff.
fn content_type_for_path(path: &std::path::Path, data: &[u8]) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" | "mdown" => "text/markdown; charset=utf-8".into(),
        "html" | "htm" => "text/html; charset=utf-8".into(),
        "css" => "text/css; charset=utf-8".into(),
        "js" | "mjs" => "text/javascript; charset=utf-8".into(),
        "json" => "application/json".into(),
        "txt" | "text" | "log" => "text/plain; charset=utf-8".into(),
        "svg" => "image/svg+xml".into(),
        "png" => "image/png".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "gif" => "image/gif".into(),
        "webp" => "image/webp".into(),
        "pdf" => "application/pdf".into(),
        "wasm" => "application/wasm".into(),
        _ => infer::get(data)
            .map(|kind| kind.mime_type().to_string())
            .unwrap_or_else(|| "application/octet-stream".into()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteFileRequest {
    path: String,
    content: String,
    #[serde(default)]
    expected_revision: i64,
}

async fn write_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<WriteFileRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    let revision = state
        .workspaces
        .write_file(
            &id,
            &request.path,
            &request.content,
            request.expected_revision,
        )
        .await
        .map_err(app_error)?;
    Ok(Json(json!({"revision": revision, "path": request.path})))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SearchQuery {
    pattern: Option<String>,
    ignore_case: Option<bool>,
    max_results: Option<i32>,
    file_pattern: Option<String>,
    context_lines: Option<i32>,
}

async fn search(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<crate::interfaces::SearchResult>>, ApiResponseError> {
    let pattern = required_query(query.pattern, "pattern")?;
    let options = SearchOptions {
        pattern: pattern.clone(),
        ignore_case: query.ignore_case.unwrap_or(false),
        max_results: query.max_results.unwrap_or_default(),
        file_pattern: query.file_pattern.unwrap_or_default(),
        context_lines: query.context_lines.unwrap_or_default(),
    };
    state
        .workspaces
        .search(&id, &pattern, options)
        .await
        .map(Json)
        .map_err(app_error)
}

#[derive(Deserialize, Default)]
struct EventsQuery {
    after: Option<i64>,
    limit: Option<i32>,
}

async fn events(
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<crate::interfaces::Event>>, ApiResponseError> {
    state
        .events
        .query_all(query.after.unwrap_or_default(), event_limit(query.limit))
        .await
        .map(Json)
        .map_err(app_error)
}

async fn session_events(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<crate::interfaces::Event>>, ApiResponseError> {
    state
        .events
        .query(
            &session_id,
            query.after.unwrap_or_default(),
            event_limit(query.limit),
        )
        .await
        .map(Json)
        .map_err(app_error)
}

async fn list_agents(
    State(state): State<AppState>,
) -> Result<Json<Vec<AgentInfo>>, ApiResponseError> {
    state.acp.list_agents().await.map(Json).map_err(app_error)
}

async fn upsert_agent(
    State(state): State<AppState>,
    body: Result<Json<AgentInfo>, JsonRejection>,
) -> Result<Json<AgentInfo>, ApiResponseError> {
    let Json(agent) = decode_json_body(body)?;
    if agent.id.trim().is_empty() || agent.command.trim().is_empty() {
        return Err(ApiResponseError::bad_request(
            "agent id and command are required",
        ));
    }
    state.acp.register_agent(agent.clone());
    let mut config = state.config.write();
    config.upsert_agent(agent.clone()).map_err(|error| {
        ApiResponseError::internal(format!("persist agent configuration: {error}"))
    })?;
    Ok(Json(agent))
}

async fn delete_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiResponseError> {
    state.acp.remove_agent(&id);
    state.config.write().delete_agent(&id).map_err(|error| {
        ApiResponseError::internal(format!("persist agent configuration: {error}"))
    })?;
    Ok(Json(json!({"status": "deleted"})))
}

async fn autodetect_agents() -> Json<Vec<AgentInfo>> {
    Json(acp::autodetect().await)
}

async fn list_sessions(State(state): State<AppState>) -> Json<Vec<crate::interfaces::SessionInfo>> {
    Json(state.acp.list_sessions())
}

async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::interfaces::SessionInfo>, ApiResponseError> {
    state.acp.get_session_info(&id).map(Json).map_err(app_error)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSessionRequest {
    agent_id: String,
    model_id: String,
    workspace_id: String,
}

async fn create_session(
    State(state): State<AppState>,
    body: Result<Json<CreateSessionRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<crate::interfaces::SessionInfo>), ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    state
        .acp
        .create_session(&request.agent_id, &request.model_id, &request.workspace_id)
        .await
        .map(|session| (StatusCode::CREATED, Json(session)))
        .map_err(app_error)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchSessionRequest {
    name: Option<String>,
    agent_id: Option<String>,
    model_id: Option<String>,
    max_transfer_bytes: Option<i64>,
}

async fn patch_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<PatchSessionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if let Some(name) = request.name {
        state.acp.rename_session(&id, &name).map_err(app_error)?;
    }

    // Model-only: switch on the live session; agent+model: full rebind.
    if request.agent_id.is_none() {
        if let Some(model_id) = request.model_id.as_deref() {
            state
                .acp
                .switch_model(&id, model_id)
                .await
                .map_err(|error| ApiResponseError::bad_request(error.to_string()))?;
            return Ok(Json(json!({"status": "updated"})));
        }
    }

    if let (Some(agent_id), Some(model_id)) =
        (request.agent_id.as_deref(), request.model_id.as_deref())
    {
        let max_transfer = request.max_transfer_bytes.unwrap_or(0);
        state
            .acp
            .rebind_session(&id, agent_id, model_id, max_transfer)
            .await
            .map_err(|error| ApiResponseError::bad_request(error.to_string()))?;
    }

    Ok(Json(json!({"status": "updated"})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptAttachment {
    id: String,
    name: String,
    mime_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptRequest {
    content: String,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    attachments: Vec<PromptAttachment>,
}

async fn send_prompt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<PromptRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if request.content.trim().is_empty() {
        return Err(ApiResponseError::bad_request("prompt content is required"));
    }

    if let Some(profile) = request
        .profile
        .as_deref()
        .filter(|profile| !profile.is_empty())
    {
        state.acp.set_session_profile(&id, profile);
    }

    let mut attachments = Vec::with_capacity(request.attachments.len());
    if !request.attachments.is_empty() {
        let uploads = state
            .uploads
            .as_ref()
            .ok_or_else(|| ApiResponseError::bad_request("uploads not configured"))?;
        let manager = uploads.lock().map_err(|_| {
            error!("uploads manager lock poisoned");
            ApiResponseError::internal("uploads manager unavailable")
        })?;
        for att in &request.attachments {
            let abs_path = manager.get(&id, &att.id).map_err(|_| {
                ApiResponseError::bad_request(format!("attachment {} not found", att.id))
            })?;
            attachments.push(Attachment {
                id: att.id.clone(),
                name: att.name.clone(),
                mime_type: att.mime_type.clone(),
                path: abs_path.display().to_string(),
                uri: format!("file://{}", abs_path.display()),
            });
        }
    }

    state
        .acp
        .send_prompt(&id, &request.content, &attachments)
        .await
        .map_err(app_error)?;
    Ok(Json(json!({"status": "sent"})))
}

async fn cancel_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiResponseError> {
    state.acp.cancel_session(&id).await.map_err(app_error)?;
    Ok(Json(json!({"status": "cancelled"})))
}

async fn close_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiResponseError> {
    state.acp.close_session(&id).await.map_err(app_error)?;
    // Best-effort upload cleanup — ACP is intentionally decoupled from uploads.
    if let Some(uploads) = &state.uploads {
        if let Ok(manager) = uploads.lock() {
            if let Err(error) = manager.remove_session(&id) {
                warn!(session_id = %id, %error, "failed to remove session uploads");
            }
        }
    }
    Ok(Json(json!({"status": "closed"})))
}

async fn pending_permissions(
    State(state): State<AppState>,
) -> Json<Vec<crate::interfaces::PermissionRequest>> {
    Json(state.permissions.get_pending())
}

#[derive(Deserialize)]
struct RespondPermissionRequest {
    decision: String,
}

async fn respond_permission(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<RespondPermissionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    let decision = serde_json::from_value::<PermissionDecision>(Value::String(request.decision))
        .map_err(|_| ApiResponseError::bad_request("invalid permission decision"))?;
    state
        .permissions
        .respond(&id, decision)
        .await
        .map_err(app_error)?;
    Ok(Json(json!({"status": "responded"})))
}

/// SPA fallback serving embedded frontend assets. Uses `OriginalUri` rather
/// than `Path<String>` so the root path `/` (zero segments) is handled without
/// a `Path` extraction error.
async fn spa_fallback(OriginalUri(uri): OriginalUri) -> Response {
    let path = uri.path().trim_start_matches('/');
    embed::serve(path.to_string()).await
}

/// Auth is deliberately performed before handlers, not in individual routes,
/// so every new protected route inherits the same LAN and CSRF policy.
async fn require_auth(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Direct Router tests lack ConnectInfo; peer_addr_string treats that as
    // loopback so host-only tests never open a LAN request.
    let remote_addr = peer_addr_string(request.extensions());
    match authorize_request(
        &state.pairing,
        &remote_addr,
        request.method(),
        request.headers(),
        request.uri().query(),
    ) {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

/// Limit unauthenticated pairing requests before they allocate QR sessions or
/// enter passcode verification. This mirrors Go's per-IP, 5/minute token bucket.
async fn require_pair_rate_limit(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let peer = request
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|connect| connect.0.ip().to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    if !allow_pair_request(&state, &peer) {
        debug!(peer, "pairing request rate limited");
        return ApiResponseError::rate_limited("pairing rate limit exceeded, try again later")
            .into_response();
    }
    next.run(request).await
}

fn allow_pair_request(state: &AppState, peer: &str) -> bool {
    let mut buckets = match state.pair_rate.lock() {
        Ok(buckets) => buckets,
        Err(poisoned) => {
            error!("pairing rate-limit lock poisoned; recovering state");
            poisoned.into_inner()
        }
    };
    let now = Instant::now();
    let bucket = buckets
        .entry(peer.to_string())
        .or_insert_with(|| PairRateBucket {
            tokens: PAIR_RATE_BURST,
            updated_at: now,
        });
    bucket.tokens = (bucket.tokens
        + now.duration_since(bucket.updated_at).as_secs_f64() * PAIR_RATE_PER_MINUTE / 60.0)
        .min(PAIR_RATE_BURST);
    bucket.updated_at = now;
    if bucket.tokens < 1.0 {
        return false;
    }
    bucket.tokens -= 1.0;
    true
}

/// Apply Go-compatible loopback bypass and Origin/credential checks.
fn authorize_request(
    pairing: &PairingManager,
    remote_addr: &str,
    method: &Method,
    headers: &HeaderMap,
    query: Option<&str>,
) -> Result<(), ApiResponseError> {
    if is_loopback_addr(remote_addr) {
        if is_mutating(method) && !loopback_origin_allowed(headers.get(header::ORIGIN)) {
            warn!(
                remote = remote_addr,
                "rejected cross-origin loopback API mutation"
            );
            return Err(ApiResponseError::forbidden(
                "cross-origin request not allowed",
            ));
        }
        return Ok(());
    }
    let (device_id, secret) = extract_credential(headers, query);
    if device_id.is_empty()
        || secret.is_empty()
        || !pairing.validate_credential(&device_id, &secret)
    {
        debug!(
            remote = remote_addr,
            "rejected unauthenticated remote API request"
        );
        return Err(ApiResponseError::unauthorized("unauthorized"));
    }
    Ok(())
}

fn is_mutating(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn loopback_origin_allowed(origin: Option<&HeaderValue>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

fn extract_credential(headers: &HeaderMap, query: Option<&str>) -> (String, String) {
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(|value| value.split_once(':'))
    {
        return (value.0.to_string(), value.1.to_string());
    }
    let url =
        query.and_then(|query| reqwest::Url::parse(&format!("http://localhost/?{query}")).ok());
    let lookup = |name: &str| {
        url.as_ref()
            .and_then(|url| {
                url.query_pairs()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.into_owned())
            })
            .unwrap_or_default()
    };
    (lookup("deviceId"), lookup("secret"))
}

fn event_limit(limit: Option<i32>) -> i32 {
    limit
        .filter(|limit| *limit > 0)
        .unwrap_or(DEFAULT_EVENT_LIMIT)
        .min(MAX_EVENT_LIMIT)
}

fn required_query(value: Option<String>, name: &str) -> Result<String, ApiResponseError> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiResponseError::bad_request(format!("missing '{name}' query parameter")))
}

/// Map Go `mustDecodeJSON` failures to a stable client-facing message.
pub(crate) fn decode_json_body<T>(
    body: Result<Json<T>, JsonRejection>,
) -> Result<Json<T>, ApiResponseError> {
    body.map_err(|_| ApiResponseError::bad_request("invalid request body"))
}

/// Configured grace window for destructive pending actions (0 = immediate).
fn revocation_grace_period(state: &AppState) -> Duration {
    let secs = state.config.read().revocation_grace_period_seconds.max(0);
    Duration::from_secs(u64::try_from(secs).unwrap_or(0))
}

/// Socket address string for loopback checks. Missing `ConnectInfo` (unit tests)
/// defaults to loopback so host-only paths stay open without LAN exposure.
fn peer_addr_string(extensions: &axum::http::Extensions) -> String {
    extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|connect| connect.0.to_string())
        .unwrap_or_else(|| "127.0.0.1:0".to_string())
}

/// Device ID from Bearer/`deviceId` query — empty on loopback-only requests.
fn device_id_from_request(headers: &HeaderMap, query: Option<&str>) -> String {
    extract_credential(headers, query).0
}

/// Shared cancel flow for grace-period pending actions (decode → cancel → event).
async fn cancel_pending_action(
    state: &AppState,
    body: Result<Json<CancelActionRequest>, JsonRejection>,
    cancel: impl FnOnce(&str) -> Result<(), PairingError>,
    event_type: EventType,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if request.action_id.is_empty() {
        return Err(ApiResponseError::bad_request("missing 'actionId'"));
    }
    cancel(&request.action_id).map_err(cancel_pending_error)?;

    let mut event = Event::new(0, event_type, "", Utc::now());
    event.request_id = request.action_id;
    record_event(state, event).await;

    Ok(Json(json!({"status": "cancelled"})))
}

/// Persist + broadcast a pending-action event; failures are logged, not fatal.
async fn record_event(state: &AppState, event: Event) {
    if let Err(error) = state.events.append_and_publish(event).await {
        warn!(%error, "failed to record pending-action event");
    }
}

/// Cancel handlers always return 404 on pairing miss/type mismatch (Go parity).
fn cancel_pending_error(error: PairingError) -> ApiResponseError {
    match error {
        PairingError::PendingActionNotFound | PairingError::PendingActionTypeMismatch => {
            ApiResponseError::not_found(error.to_string())
        }
        other => pairing_error(other),
    }
}

fn pairing_error(error: PairingError) -> ApiResponseError {
    match error {
        PairingError::InvalidPasscode | PairingError::InvalidToken => {
            ApiResponseError::unauthorized(error.to_string())
        }
        PairingError::RateLimited => ApiResponseError::rate_limited(error.to_string()),
        PairingError::DeviceNotFound(_) | PairingError::PendingActionNotFound => {
            ApiResponseError::not_found(error.to_string())
        }
        PairingError::PendingActionTypeMismatch | PairingError::DuplicatePendingAction => {
            ApiResponseError::bad_request(error.to_string())
        }
        PairingError::Persistence(_)
        | PairingError::State(_)
        | PairingError::Qr(_)
        | PairingError::QrEncoding(_) => {
            error!(%error, "pairing API operation failed");
            ApiResponseError::internal("pairing operation failed")
        }
    }
}

fn app_error(error: AppError) -> ApiResponseError {
    let mapped = map_api_error(&error);
    let status = StatusCode::from_u16(mapped.status.0).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    ApiResponseError {
        status,
        message: mapped.body.error,
    }
}

pub(crate) struct ApiResponseError {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
}

impl ApiResponseError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn rate_limited(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: message.into(),
        }
    }

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiResponseError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": self.message}))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use tower::ServiceExt;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("temporary state directory");
        let config = ConfigStore::new(crate::config::Config {
            data_dir: dir.path().display().to_string(),
            db_path: dir.path().join("events.db").display().to_string(),
            ..crate::config::Config::default()
        });
        let pairing = PairingManager::new(dir.path(), None).expect("pairing manager");
        let workspaces = Arc::new(WorkspaceManagerImpl::new());
        let events = Arc::new(
            crate::events::EventBus::open(dir.path().join("events.db")).expect("event bus"),
        );
        let hub = Hub::with_event_bus(Arc::clone(&events));
        let permissions = PermissionsManager::new(None);
        let registry = Arc::new(crate::acp::AgentRegistry::default());
        let acp = Arc::new(Client::new(crate::acp::ClientDeps {
            registry,
            workspaces: workspaces.clone(),
            permissions: permissions.clone(),
            event_bus: events.clone(),
            conversation_store: crate::acp::ConversationStore::new(None),
            mcp_config_path: None,
        }));
        (
            dir,
            AppState::new(
                config,
                pairing,
                workspaces,
                events,
                hub,
                acp,
                permissions,
                None,
                None,
                None,
            ),
        )
    }

    /// Pair a device and optionally set grace / remote-registration flags.
    fn pending_actions_state(
        grace_seconds: i64,
        allow_remote: bool,
    ) -> (
        tempfile::TempDir,
        AppState,
        crate::interfaces::DeviceCredential,
    ) {
        let (dir, state) = state();
        {
            let mut cfg = state.config.write();
            cfg.revocation_grace_period_seconds = grace_seconds;
            cfg.allow_remote_workspace_registration = allow_remote;
        }
        let session = state
            .pairing
            .create_session("localhost", 7337)
            .expect("pairing session");
        let cred = state
            .pairing
            .verify_passcode(&session.passcode, "Device")
            .expect("pair device");
        (dir, state, cred)
    }

    async fn oneshot(state: AppState, request: Request<Body>) -> Response {
        oneshot_peer(state, request, "127.0.0.1:9").await
    }

    async fn oneshot_peer(state: AppState, mut request: Request<Body>, peer: &str) -> Response {
        let addr: SocketAddr = peer.parse().expect("peer address");
        request.extensions_mut().insert(ConnectInfo(addr));
        router(state).oneshot(request).await.expect("response")
    }

    fn bearer(cred: &crate::interfaces::DeviceCredential) -> HeaderValue {
        HeaderValue::from_str(&format!("Bearer {}:{}", cred.id, cred.secret))
            .expect("authorization header")
    }

    async fn json_body(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[tokio::test]
    async fn health_is_public_and_router_compiles() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn remote_request_without_a_credential_is_rejected() {
        let (_dir, state) = state();
        let headers = HeaderMap::new();
        let error = authorize_request(
            &state.pairing,
            "192.168.1.2:9000",
            &Method::GET,
            &headers,
            None,
        )
        .expect_err("missing remote credential must fail");
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn pair_request_bucket_allows_a_five_request_burst() {
        let (_dir, state) = state();
        for _ in 0..5 {
            assert!(allow_pair_request(&state, "192.168.1.2"));
        }
        assert!(!allow_pair_request(&state, "192.168.1.2"));
    }

    #[tokio::test]
    async fn pairing_rate_limit_is_exposed_as_a_client_error() {
        let (_dir, state) = state();
        for _ in 0..5 {
            let _ = state.pairing.verify_passcode("invalid", "device");
        }
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/pair/verify-passcode")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"passcode":"invalid","deviceName":"device"}"#,
            ))
            .expect("request");
        let response = oneshot(state, request).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    // --- Grace-period pending action handlers (mirrors Go api_test.go) ---

    #[tokio::test]
    async fn revoke_device_grace_period_returns_accepted_and_lists() {
        let (_dir, state, cred) = pending_actions_state(300, false);
        let response = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/devices/{}", cred.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let info = json_body(response).await;
        assert_eq!(info["deviceId"], cred.id);
        assert_eq!(info["type"], "revocation");

        let list = oneshot(
            state,
            Request::builder()
                .uri("/api/pending-actions")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(list.status(), StatusCode::OK);
        let pending = json_body(list).await;
        let pending = pending.as_array().expect("pending array");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0]["id"], info["id"]);
    }

    #[tokio::test]
    async fn revoke_device_immediate_returns_ok() {
        let (_dir, state, cred) = pending_actions_state(0, false);
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/devices/{}", cred.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["status"], "revoked");
    }

    #[tokio::test]
    async fn cancel_revocation_removes_pending_action() {
        let (_dir, state, cred) = pending_actions_state(300, false);
        let del = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/devices/{}", cred.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(del.status(), StatusCode::ACCEPTED);
        let info = json_body(del).await;
        let action_id = info["id"].as_str().expect("action id").to_owned();

        let cancel = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri("/api/devices/cancel-revocation")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"actionId":"{action_id}"}}"#)))
                .expect("request"),
        )
        .await;
        assert_eq!(cancel.status(), StatusCode::OK);

        let list = oneshot(
            state,
            Request::builder()
                .uri("/api/pending-actions")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        let pending = json_body(list).await;
        assert_eq!(pending.as_array().map(Vec::len).unwrap_or(1), 0);
    }

    #[tokio::test]
    async fn cancel_revocation_not_found_returns_404() {
        let (_dir, state, _cred) = pending_actions_state(300, false);
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/devices/cancel-revocation")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"actionId":"nonexistent"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn cancel_revocation_bad_body_returns_400() {
        let (_dir, state, _cred) = pending_actions_state(300, false);
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/devices/cancel-revocation")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{not json"))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(body["error"], "invalid request body");
    }

    #[tokio::test]
    async fn list_pending_actions_empty_ok() {
        let (_dir, state, _cred) = pending_actions_state(300, false);
        let response = oneshot(
            state,
            Request::builder()
                .uri("/api/pending-actions")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body.as_array().map(Vec::len).unwrap_or(1), 0);
    }

    #[tokio::test]
    async fn workspace_registration_disabled_returns_403() {
        let (_dir, state, cred) = pending_actions_state(300, false);
        let response = oneshot_peer(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/workspaces")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, bearer(&cred))
                .body(Body::from(r#"{"path":"/some/path"}"#))
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = json_body(response).await;
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("Remote workspace registration is disabled"),
            "body: {body}"
        );
    }

    #[tokio::test]
    async fn workspace_registration_enabled_returns_accepted() {
        let (_dir, state, cred) = pending_actions_state(300, true);
        let response = oneshot_peer(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/workspaces")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, bearer(&cred))
                .body(Body::from(r#"{"path":"/some/path"}"#))
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let info = json_body(response).await;
        assert_eq!(info["type"], "workspace_registration");
        assert_eq!(info["path"], "/some/path");
    }

    #[tokio::test]
    async fn cancel_workspace_registration_removes_pending_action() {
        let (_dir, state, cred) = pending_actions_state(300, true);
        let reg = oneshot_peer(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri("/api/workspaces")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, bearer(&cred))
                .body(Body::from(r#"{"path":"/some/path"}"#))
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(reg.status(), StatusCode::ACCEPTED);
        let info = json_body(reg).await;
        let action_id = info["id"].as_str().expect("action id").to_owned();

        let cancel = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri("/api/workspaces/cancel-registration")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"actionId":"{action_id}"}}"#)))
                .expect("request"),
        )
        .await;
        assert_eq!(cancel.status(), StatusCode::OK);

        let list = oneshot(
            state,
            Request::builder()
                .uri("/api/pending-actions")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        let pending = json_body(list).await;
        assert_eq!(pending.as_array().map(Vec::len).unwrap_or(1), 0);
    }

    #[tokio::test]
    async fn cancel_workspace_registration_bad_body_returns_400() {
        let (_dir, state, _cred) = pending_actions_state(300, true);
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/workspaces/cancel-registration")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{not json"))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(body["error"], "invalid request body");
    }
}
