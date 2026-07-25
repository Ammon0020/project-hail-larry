//! HTTP API composition and first UI-smoke handlers.
//!
//! The router deliberately keeps security policy at the edge: pairing is the
//! only unauthenticated API, loopback requests bypass device credentials with
//! an Origin check on mutations, and the WebSocket hub performs its own
//! browser-specific credential and Origin gate.

mod embed;
mod mcp;
mod profiles;
mod providers;
mod session_extra;
mod settings;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{
    ConnectInfo, DefaultBodyLimit, FromRequestParts, OriginalUri, Path, Query, State,
};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use chrono::Utc;
use rand::Rng;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, error, warn};

use crate::acp::{self, Client};
use crate::config::{AgentInfo, ConfigStore};
use crate::events::SharedEventBus;
use crate::interfaces::{
    map_api_error, ACPClient, AppError, Attachment, Event, EventStore, EventType,
    PermissionDecision, PermissionManager, SearchOptions, WorkspaceManager,
    PENDING_ACTION_TYPE_REVOCATION,
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

/// Per-field size limits for agent config (defense-in-depth on top of loopback gate).
const MAX_AGENT_COMMAND_LEN: usize = 1024;
const MAX_AGENT_ARGS_COUNT: usize = 64;
const MAX_AGENT_MODELS_COUNT: usize = 256;

const MAX_EVENT_LIMIT: i32 = 10_000;
const DEFAULT_EVENT_LIMIT: i32 = 1_000;
const PAIR_RATE_PER_MINUTE: f64 = 5.0;
const PAIR_RATE_BURST: f64 = 5.0;
/// Idle window after which a per-IP bucket has fully refilled to `PAIR_RATE_BURST`
/// and can be evicted without changing observable rate-limit behavior.
const PAIR_RATE_IDLE_TTL: Duration = Duration::from_secs(60);
/// Only run the eviction pass once the map grows past this size, so a healthy
/// daemon pays no O(n) cost on every pairing request while still bounding
/// memory under a many-source-IP flood.
const PAIR_RATE_EVICT_THRESHOLD: usize = 1024;
const PREVIEW_TOKEN_TTL: Duration = Duration::from_secs(30 * 60);

/// Maximum characters allowed in a paired device name.
const MAX_DEVICE_NAME_CHARS: usize = 64;
/// Maximum characters allowed in a session name.
const MAX_SESSION_NAME_CHARS: usize = 128;
/// HTML-significant characters rejected in device names to prevent stored XSS
/// when the name is rendered in the browser UI.
const HTML_SIGNIFICANT_CHARS: &[char] = &['<', '>', '&', '"', '\''];

/// Validates a paired device name: non-empty, within the length cap, free of
/// control characters and HTML-significant characters. Device names are
/// attacker-controlled (any paired device can set one) and may be rendered in
/// the UI, so both DoS and stored-XSS surfaces are bounded here.
fn validate_device_name(name: &str) -> Result<(), ApiResponseError> {
    if name.is_empty() {
        return Err(ApiResponseError::bad_request(
            "device name must not be empty",
        ));
    }
    let len = name.chars().count();
    if len > MAX_DEVICE_NAME_CHARS {
        return Err(ApiResponseError::bad_request(format!(
            "device name exceeds {MAX_DEVICE_NAME_CHARS} characters"
        )));
    }
    if let Some(ch) = name
        .chars()
        .find(|c| *c < ' ' || HTML_SIGNIFICANT_CHARS.contains(c))
    {
        return Err(ApiResponseError::bad_request(format!(
            "device name contains forbidden character `{ch}`"
        )));
    }
    Ok(())
}

/// Validates a session name: within the length cap and free of control
/// characters. Session names are client-controlled and may be displayed in the
/// UI, so the DoS surface is bounded and control characters are rejected.
fn validate_session_name(name: &str) -> Result<(), ApiResponseError> {
    let len = name.chars().count();
    if len > MAX_SESSION_NAME_CHARS {
        return Err(ApiResponseError::bad_request(format!(
            "session name exceeds {MAX_SESSION_NAME_CHARS} characters"
        )));
    }
    if name.chars().any(|c| c < ' ') {
        return Err(ApiResponseError::bad_request(
            "session name contains forbidden control character",
        ));
    }
    Ok(())
}

/// Marks a request that arrived over the daemon's native TLS listener.
///
/// The listener adds this before routing so security-sensitive handlers do not
/// infer transport security from a client-controlled header.
#[derive(Clone, Copy)]
pub(crate) struct TlsConnection;

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
    /// In-memory, workspace-scoped preview tickets with short expiry.
    preview_tokens: Arc<Mutex<HashMap<String, PreviewToken>>>,
    /// Cached autodetect results with a cooldown to prevent probe-spawn DoS.
    autodetect_cache: AutodetectCache,
}

/// Cached autodetect results: `(timestamp, agent list)`.
type AutodetectCache = Arc<Mutex<Option<(Instant, Vec<AgentInfo>)>>>;

/// Per-IP token bucket matching Go's five-request burst and 5/minute refill.
struct PairRateBucket {
    tokens: f64,
    updated_at: Instant,
}

struct PreviewToken {
    workspace_id: String,
    expires_at: Instant,
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
            preview_tokens: Arc::new(Mutex::new(HashMap::new())),
            autodetect_cache: Arc::new(Mutex::new(None)),
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
        .route(
            "/api/workspaces/{id}/file",
            get(read_file).delete(delete_file),
        )
        .route("/api/workspaces/{id}/raw", get(raw_file))
        .route(
            "/api/workspaces/{id}/preview-session",
            post(create_preview_session),
        )
        // Browse-preview virtual root: path-based so relative asset URLs resolve.
        // Bare `/preview/{id}` (no file) returns 404 — no SPA fallback.
        .route("/preview/{id}", get(preview_empty))
        .route("/preview/{id}/{*path}", get(preview_file))
        .route("/api/workspaces/{id}/search", get(search))
        .route("/api/workspaces/{id}/rename", post(rename_path))
        .route("/api/workspaces/{id}/mkdir", post(mkdir))
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
        .route("/api/sessions/{id}/profile", post(set_session_profile))
        .route(
            "/api/sessions/{id}/export",
            get(session_extra::export_session),
        )
        .route(
            "/api/sessions/{id}/capabilities",
            get(session_extra::session_capabilities),
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
        .route(
            "/api/profiles",
            get(profiles::get_profiles).put(profiles::put_profiles),
        )
        .route(
            "/api/settings/prompt-context",
            get(settings::get_prompt_context).put(settings::put_prompt_context),
        )
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
        .layer(middleware::from_fn(security_headers))
}

/// Global response hardening applied to every route.
///
/// `X-Content-Type-Options` and `X-Frame-Options` defend all responses against
/// MIME sniffing and clickjacking of the IDE shell. HSTS is gated on the
/// `TlsConnection` extension (set only by the HTTPS listener) so cleartext
/// responses never pin a host that was reached over HTTP. These three headers
/// are not set by any per-route handler, so `insert` cannot clobber the preview
/// CSP or raw Referrer-Policy that handlers attach themselves.
///
/// A restrictive CSP is applied to the SPA shell and API responses as
/// defense-in-depth against XSS. It is only inserted when the handler has not
/// already set its own CSP (e.g. the preview handler uses `sandbox
/// allow-same-origin` for agent-written HTML). `style-src 'unsafe-inline'` is
/// required because CodeMirror injects inline styles; no `'unsafe-inline'` is
/// granted to `script-src`.
async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let over_tls = request.extensions().get::<TlsConnection>().is_some();
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static("SAMEORIGIN"),
    );
    if over_tls {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=63072000; includeSubDomains"),
        );
    }
    // Only set the SPA CSP when the handler hasn't already attached one
    // (e.g. the preview endpoint sets `sandbox allow-same-origin`).
    if !headers.contains_key(header::CONTENT_SECURITY_POLICY) {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self'; connect-src 'self' ws: wss:; \
                 img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; \
                 frame-src 'self'",
            ),
        );
    }
    response
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Result<Json<PairVerifyRequest>, JsonRejection>,
) -> Result<Json<crate::interfaces::DeviceCredential>, ApiResponseError> {
    let peer_key = pair_rate_key(&addr.ip());
    pair_verify(state, body, |pairing, request| {
        pairing.verify_passcode(
            request.passcode.as_deref().unwrap_or_default(),
            request.device_name,
            Some(&peer_key),
        )
    })
}

async fn pair_verify_token(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Result<Json<PairVerifyRequest>, JsonRejection>,
) -> Result<Json<crate::interfaces::DeviceCredential>, ApiResponseError> {
    let peer_key = pair_rate_key(&addr.ip());
    pair_verify(state, body, |pairing, request| {
        pairing.verify_token(
            request.token.as_deref().unwrap_or_default(),
            request.device_name,
            Some(&peer_key),
        )
    })
}

/// Shared decode + verify path for passcode and QR-token pairing.
fn pair_verify(
    state: AppState,
    body: Result<Json<PairVerifyRequest>, JsonRejection>,
    verify: impl FnOnce(
        &PairingManager,
        PairVerifyRequest,
    ) -> Result<crate::interfaces::DeviceCredential, PairingError>,
) -> Result<Json<crate::interfaces::DeviceCredential>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    // Validate the attacker-controlled device name before it reaches pairing
    // state (DoS / stored-XSS guard).
    validate_device_name(&request.device_name)?;
    verify(&state.pairing, request)
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
) -> Result<Response, ApiResponseError> {
    let grace = revocation_grace_period(&state);
    let requester = device_id_from_request(&headers);
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
///
/// A device that is the target of a pending revocation must not be able to
/// cancel its own revocation, otherwise it can indefinitely prevent its own
/// removal during the grace period. The host (loopback, no device id) and any
/// other paired device may still cancel — matching the consensus model where
/// any paired device can act on a pending action.
async fn cancel_revocation(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CancelActionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let request = decode_json_body(body)?.0;
    let caller = device_id_from_request(&headers);
    if !caller.is_empty() {
        if let Some(action) = state
            .pairing
            .list_pending_actions()
            .into_iter()
            .find(|a| a.id == request.action_id)
        {
            if action.action_type == PENDING_ACTION_TYPE_REVOCATION && action.device_id == caller {
                return Err(ApiResponseError::forbidden(
                    "a device cannot cancel its own revocation",
                ));
            }
        }
    }
    cancel_pending_action(
        &state,
        request,
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

/// `POST /api/workspaces` — gated by `allowRemoteWorkspaceRegistration`.
/// Loopback does NOT bypass this policy (Go parity): host registration stays on
/// `app add-folder` writing config; the HTTP surface stays closed when remote
/// registration is disabled — including from 127.0.0.1 (contract harness).
async fn register_workspace(
    State(state): State<AppState>,
    PeerAddr(remote_addr): PeerAddr,
    headers: HeaderMap,
    body: Result<Json<RegisterWorkspaceRequest>, JsonRejection>,
) -> Result<Response, ApiResponseError> {
    let Json(payload) = decode_json_body(body)?;
    let loopback = is_loopback_addr(&remote_addr);
    let allow_remote = state.config.read().allow_remote_workspace_registration;
    // No loopback exception: Go returns 403 unconditionally when the flag is
    // false; CLI add-folder persists config rather than calling this endpoint.
    if !allow_remote {
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

    let requester = device_id_from_request(&headers);
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
        decode_json_body(body)?.0,
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
    serve_workspace_file(&state, &id, &path).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewSessionResponse {
    token: String,
    expires_in_seconds: u64,
}

/// Creates a one-time ticket used only to bootstrap a preview cookie.
async fn create_preview_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PreviewSessionResponse>, ApiResponseError> {
    let exists = state
        .workspaces
        .list()
        .await
        .map_err(app_error)?
        .into_iter()
        .any(|workspace| workspace.id == id);
    if !exists {
        return Err(ApiResponseError::not_found(format!(
            "workspace {id} not found"
        )));
    }
    let token = new_preview_token();
    let now = Instant::now();
    let mut tokens = state
        .preview_tokens
        .lock()
        .map_err(|_| ApiResponseError::internal("preview token store lock poisoned"))?;
    tokens.retain(|_, ticket| ticket.expires_at > now);
    tokens.insert(
        token.clone(),
        PreviewToken {
            workspace_id: id,
            expires_at: now + PREVIEW_TOKEN_TTL,
        },
    );
    Ok(Json(PreviewSessionResponse {
        token,
        expires_in_seconds: PREVIEW_TOKEN_TTL.as_secs(),
    }))
}

/// Creates a 256-bit opaque value for a preview ticket or cookie.
fn new_preview_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// 404 for `/preview/{id}` with no file path — never fall through to the SPA.
async fn preview_empty() -> Result<Response, ApiResponseError> {
    Err(ApiResponseError::not_found("preview path required"))
}

/// Serves a workspace file at `/preview/{id}/{*path}` so relative URLs in an
/// iframe (CSS/JS/images) resolve against the preview root rather than a
/// query-string raw endpoint.
async fn preview_file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<Response, ApiResponseError> {
    // Trailing slash / empty wildcard → no file to serve.
    let rel = path.trim_matches('/');
    if rel.is_empty() {
        return Err(ApiResponseError::not_found("preview path required"));
    }
    serve_workspace_file(&state, &id, rel)
        .await
        .map(|mut response| {
            // Preview HTML may run scripts in a sandboxed iframe; confine who
            // can embed these responses (IDE only, same origin) and neutralize
            // scripts so agent-written content cannot execute with IDE-origin
            // authority (same-origin XSS). `sandbox` without `allow-scripts`
            // blocks script execution; `allow-same-origin` preserves relative
            // subresource loading for legitimate static previews.
            response.headers_mut().insert(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static("frame-ancestors 'self'; sandbox allow-same-origin"),
            );
            response
        })
}

/// Shared body for `/raw` and `/preview`: resolve via workspace containment,
/// read bytes, set Content-Type + inline disposition.
async fn serve_workspace_file(
    state: &AppState,
    workspace_id: &str,
    rel_path: &str,
) -> Result<Response, ApiResponseError> {
    let absolute = state
        .workspaces
        .file_path(workspace_id, rel_path)
        .await
        .map_err(app_error)?;
    let data = tokio::fs::read(&absolute)
        .await
        .map_err(|error| ApiResponseError::internal(format!("read workspace file: {error}")))?;
    // Prefer extension so README.md is text/markdown; charset=utf-8 for text.
    let content_type = content_type_for_path(std::path::Path::new(&absolute), &data);
    let mut response = Response::new(Body::from(data));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
    // Avoid leaking deviceId/secret query params via Referer when the preview
    // HTML loads third-party (or cross-path) subresources.
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    // Neutralize scripts in agent-written HTML served same-origin with the IDE:
    // without this, a workspace `evil.html` could call authenticated `/api/*`
    // endpoints and read IDE cookies/localStorage. `sandbox` without
    // `allow-scripts` blocks script execution; `allow-same-origin` preserves
    // relative subresource loading for legitimate static previews. The preview
    // handler extends this with `frame-ancestors 'self'`.
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox allow-same-origin"),
    );
    let content_type = HeaderValue::try_from(content_type).map_err(|error| {
        ApiResponseError::internal(format!("derive file content type: {error}"))
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

/// `DELETE /api/workspaces/{id}/file?path=` — remove a file or empty directory.
async fn delete_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Json<Value>, ApiResponseError> {
    let path = required_query(query.path, "path")?;
    state
        .workspaces
        .delete_path(&id, &path)
        .await
        .map_err(app_error)?;
    Ok(Json(json!({"status": "deleted"})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenamePathRequest {
    from: String,
    to: String,
}

/// `POST /api/workspaces/{id}/rename` — rename/move within the workspace.
/// Overwrite is rejected with 409 when the destination already exists.
async fn rename_path(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<RenamePathRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if request.from.is_empty() || request.to.is_empty() {
        return Err(ApiResponseError::bad_request(
            "from and to paths are required",
        ));
    }
    state
        .workspaces
        .rename_path(&id, &request.from, &request.to)
        .await
        .map_err(app_error)?;
    Ok(Json(json!({"status": "renamed"})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MkdirRequest {
    path: String,
}

/// `POST /api/workspaces/{id}/mkdir` — create a directory (parents as needed).
/// Idempotent if the path already exists as a directory; 409 if it is a file.
async fn mkdir(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<MkdirRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if request.path.is_empty() {
        return Err(ApiResponseError::bad_request("path is required"));
    }
    state
        .workspaces
        .mkdir(&id, &request.path)
        .await
        .map_err(app_error)?;
    Ok(Json(json!({"status": "created"})))
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

async fn delete_agent(
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

/// Cooldown for autodetect probe spawning to prevent resource exhaustion from
// repeated calls. Each autodetect spawns up to 5 child processes.
const AUTODETECT_COOLDOWN: Duration = Duration::from_secs(60);

async fn autodetect_agents(State(state): State<AppState>) -> Json<Vec<AgentInfo>> {
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
    profile_id: Option<String>,
}

async fn create_session(
    State(state): State<AppState>,
    body: Result<Json<CreateSessionRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<crate::interfaces::SessionInfo>), ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    state
        .acp
        .create_session_with_profile(
            &request.agent_id,
            &request.model_id,
            &request.workspace_id,
            request.profile_id.as_deref(),
        )
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
        // Validate the client-supplied session name before persisting it
        // (DoS / control-character guard).
        validate_session_name(&name)?;
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

    // Profile selection moved to POST /api/sessions/{id}/profile (S-PROF-ACP).
    // The `profile` field is intentionally no longer read here; sending one is
    // silently ignored by serde (the field is absent from PromptRequest).

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

/// Request body for `POST /api/sessions/{id}/profile`.
#[derive(Deserialize)]
struct SetProfileRequest {
    /// Profile id (validated against the loaded config; unknown → 400).
    profile: String,
}

/// `POST /api/sessions/{id}/profile` — set the active profile for a session.
///
/// Replaces the deprecated `profile` field on `/prompt`. Validates the profile
/// id, stores the selection in the profile middleware, and pushes it to the
/// agent over ACP (`session/set_config_option`, mode category) when the agent
/// advertised the capability. Auth-gated by the protected router.
async fn set_session_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<SetProfileRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if request.profile.trim().is_empty() {
        return Err(ApiResponseError::bad_request("profile is required"));
    }
    state
        .acp
        .set_session_profile(&id, &request.profile)
        .await
        .map_err(app_error)?;
    Ok(Json(json!({"status": "updated"})))
}

async fn close_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiResponseError> {
    state.acp.close_session(&id).await.map_err(app_error)?;
    // Best-effort upload cleanup — ACP is intentionally decoupled from uploads.
    if let Some(uploads) = &state.uploads {
        if let Ok(mut manager) = uploads.lock() {
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
///
/// Paths under `/preview/` never fall through to the SPA: browsers/clients
/// may normalize `../` out of a preview URL (e.g. `/preview/id/../../../etc/passwd`
/// → `/etc/passwd` or similar), and serving the IDE shell there would mask
/// traversal attempts. Unmatched preview URLs are 404.
async fn spa_fallback(OriginalUri(uri): OriginalUri) -> Response {
    let path = uri.path();
    if path == "/preview" || path.starts_with("/preview/") {
        return ApiResponseError::not_found("preview path not found").into_response();
    }
    let path = path.trim_start_matches('/');
    embed::serve(path.to_string()).await
}

/// Auth is deliberately performed before handlers, not in individual routes,
/// so every new protected route inherits the same LAN and CSRF policy.
async fn require_auth(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // peer_addr_string fails closed (non-loopback) when ConnectInfo is
    // absent in production; tests insert it via `oneshot_peer`.
    let remote_addr = peer_addr_string(request.extensions());
    match authorize_request(
        &state.pairing,
        &remote_addr,
        request.method(),
        request.headers(),
    ) {
        Ok(()) => next.run(request).await,
        Err(error) if request.method() == Method::GET || request.method() == Method::HEAD => {
            let Some(preview_auth) =
                preview_authorization(&state, request.uri(), request.headers())
            else {
                return error.into_response();
            };
            let secure = request.extensions().get::<TlsConnection>().is_some();
            let mut response = next.run(request).await;
            if let Some(token) = preview_auth.cookie_token {
                let mut cookie = format!(
                    "preview_token={token}; Path=/preview/{}/; HttpOnly; SameSite=Lax; Max-Age={}",
                    preview_auth.workspace_id,
                    PREVIEW_TOKEN_TTL.as_secs()
                );
                if secure {
                    cookie.push_str("; Secure");
                }
                if let Ok(value) = HeaderValue::from_str(&cookie) {
                    response.headers_mut().append(header::SET_COOKIE, value);
                }
            }
            response
        }
        Err(error) => error.into_response(),
    }
}

struct PreviewAuthorization {
    workspace_id: String,
    /// A fresh cookie value when an entry ticket was consumed.
    cookie_token: Option<String>,
}

/// Authenticates a preview read and exchanges an entry ticket for a cookie.
///
/// Query tickets are intentionally one-time: URLs may reach browser history or
/// server logs, while the replacement value is HttpOnly and path-scoped.
fn preview_authorization(
    state: &AppState,
    uri: &Uri,
    headers: &HeaderMap,
) -> Option<PreviewAuthorization> {
    let mut segments = uri.path().split('/').filter(|segment| !segment.is_empty());
    if segments.next()? != "preview" {
        return None;
    }
    let workspace_id = segments.next()?.to_string();
    let entry_token = uri.query().and_then(|query| {
        query.split('&').find_map(|part| {
            let (name, value) = part.split_once('=')?;
            (name == "previewToken").then_some(value)
        })
    });
    if let Some(token) = entry_token {
        return exchange_preview_ticket(state, &workspace_id, token).map(|cookie_token| {
            PreviewAuthorization {
                workspace_id,
                cookie_token: Some(cookie_token),
            }
        });
    }
    let cookie_token = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                (name == "preview_token").then_some(value)
            })
        });
    let token = cookie_token?;
    let tokens = state.preview_tokens.lock().ok()?;
    let ticket = tokens.get(token)?;
    (ticket.expires_at > Instant::now() && ticket.workspace_id == workspace_id).then_some(
        PreviewAuthorization {
            workspace_id,
            cookie_token: None,
        },
    )
}

/// Consumes an entry ticket and returns a fresh cookie token for its workspace.
fn exchange_preview_ticket(
    state: &AppState,
    workspace_id: &str,
    entry_token: &str,
) -> Option<String> {
    let now = Instant::now();
    let mut tokens = state.preview_tokens.lock().ok()?;
    tokens.retain(|_, ticket| ticket.expires_at > now);
    let expires_at = match tokens.get(entry_token) {
        Some(ticket) if ticket.workspace_id == workspace_id => ticket.expires_at,
        _ => return None,
    };
    tokens.remove(entry_token);
    let cookie_token = new_preview_token();
    tokens.insert(
        cookie_token.clone(),
        PreviewToken {
            workspace_id: workspace_id.to_owned(),
            expires_at,
        },
    );
    Some(cookie_token)
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
        .map(|connect| pair_rate_key(&connect.0.ip()))
        .unwrap_or_else(|| "127.0.0.1".to_string());
    if !allow_pair_request(&state, &peer) {
        debug!(peer, "pairing request rate limited");
        return ApiResponseError::rate_limited("pairing rate limit exceeded, try again later")
            .into_response();
    }
    next.run(request).await
}

/// Normalize a peer IP for pair-rate-limit bucketing. IPv6 addresses are
/// collapsed to their /64 prefix so an attacker rotating within a single
/// allocated subnet cannot mint a fresh bucket per /128 address. IPv4 keeps
/// the full address (its address space is small enough that per-IP buckets
/// are meaningful).
fn pair_rate_key(ip: &std::net::IpAddr) -> String {
    match ip {
        std::net::IpAddr::V6(addr) => {
            let octets = addr.octets();
            let mut prefix = [0_u8; 16];
            prefix[..8].copy_from_slice(&octets[..8]);
            std::net::Ipv6Addr::from(prefix).to_string()
        }
        std::net::IpAddr::V4(_) => ip.to_string(),
    }
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
    // Evict buckets idle beyond the refill window. A bucket idle for >= 60s has
    // refilled to PAIR_RATE_BURST, so re-creating it on the next request is
    // equivalent. Threshold-gated to avoid O(n) retain on every call.
    if buckets.len() > PAIR_RATE_EVICT_THRESHOLD {
        buckets.retain(|_, b| now.duration_since(b.updated_at) < PAIR_RATE_IDLE_TTL);
    }
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
    let (device_id, secret) = extract_credential(headers);
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
    // Loopback Origin allowlist. `0.0.0.0` is intentionally excluded: it is
    // a wildcard, not a loopback address, and is not listed in
    // `is_loopback_addr` (src/sync/mod.rs). A page whose origin is
    // `http://0.0.0.0:<port>` should connect via `127.0.0.1`/`localhost`
    // instead.
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

fn extract_credential(headers: &HeaderMap) -> (String, String) {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(|value| value.split_once(':'))
        .map(|(id, secret)| (id.to_string(), secret.to_string()))
        .unwrap_or_default()
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

/// Socket address string for loopback checks. Missing `ConnectInfo` is a
/// misconfiguration signal: in production we fail closed by returning a
/// non-loopback address so `authorize_request` requires a device credential
/// rather than silently treating the request as trusted loopback. Tests
/// insert `ConnectInfo` explicitly via `oneshot_peer`, but the loopback
/// fallback is kept under `cfg(test)` as defense-in-depth.
fn peer_addr_string(extensions: &axum::http::Extensions) -> String {
    extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|connect| connect.0.to_string())
        .unwrap_or_else(|| {
            if cfg!(test) {
                "127.0.0.1:0".to_string()
            } else {
                // Fail closed: a non-loopback address forces credential checks.
                "0.0.0.0:0".to_string()
            }
        })
}

/// Device ID from the `Authorization: Bearer` header — empty on loopback-only
/// requests (where `authorize_request` bypasses credential checks) or when no
/// bearer credential is present.
fn device_id_from_request(headers: &HeaderMap) -> String {
    extract_credential(headers).0
}

/// Shared cancel flow for grace-period pending actions (cancel → event). The
/// body is decoded by the caller so handlers can perform authorization checks
/// against the action id before cancelling.
async fn cancel_pending_action(
    state: &AppState,
    request: CancelActionRequest,
    cancel: impl FnOnce(&str) -> Result<(), PairingError>,
    event_type: EventType,
) -> Result<Json<Value>, ApiResponseError> {
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
    /// Shared constructor for status + message pairs used by handlers.
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, message)
    }

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
    }

    /// 413 — request body exceeded an endpoint-specific size cap.
    pub(crate) fn payload_too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, message)
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl IntoResponse for ApiResponseError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": self.message}))).into_response()
    }
}

/// Shared test-state construction for API handler tests.
///
/// Every handler test module needs the same 7 core deps (config, pairing,
/// workspaces, events, hub, permissions, acp). Extracting them here prevents
/// the construction block from being copied per-module — a new `AppState`
/// field only needs to be wired in one place.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::path::{Path, PathBuf};

    /// The 7 core service deps every handler test needs, built from a temp dir.
    struct CoreDeps {
        config: ConfigStore,
        pairing: PairingManager,
        workspaces: Arc<WorkspaceManagerImpl>,
        events: SharedEventBus,
        hub: Arc<Hub>,
        acp: Arc<Client>,
        permissions: Arc<PermissionsManager>,
    }

    /// Build the 7 core service deps from a temp state dir.
    fn core_deps(dir: &Path) -> CoreDeps {
        let config = ConfigStore::new(crate::config::Config {
            data_dir: dir.display().to_string(),
            db_path: dir.join("events.db").display().to_string(),
            ..crate::config::Config::default()
        });
        let pairing = PairingManager::new(dir, None).expect("pairing");
        let workspaces = Arc::new(WorkspaceManagerImpl::new());
        let events =
            Arc::new(crate::events::EventBus::open(dir.join("events.db")).expect("event bus"));
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
        CoreDeps {
            config,
            pairing,
            workspaces,
            events,
            hub,
            acp,
            permissions,
        }
    }

    /// AppState with all optional fields set to `None`.
    pub(crate) fn test_state(dir: &Path) -> AppState {
        let d = core_deps(dir);
        AppState::new(
            d.config,
            d.pairing,
            d.workspaces,
            d.events,
            d.hub,
            d.acp,
            d.permissions,
            None,
            None,
            None,
        )
    }

    /// AppState with `mcp_config_path` set (for MCP handler tests).
    pub(crate) fn test_state_with_mcp(dir: &Path, mcp_path: PathBuf) -> AppState {
        let d = core_deps(dir);
        AppState::new(
            d.config,
            d.pairing,
            d.workspaces,
            d.events,
            d.hub,
            d.acp,
            d.permissions,
            Some(mcp_path),
            None,
            None,
        )
    }

    /// AppState with an `uploads` manager (for session upload tests).
    pub(crate) fn test_state_with_uploads(dir: &Path) -> AppState {
        let d = core_deps(dir);
        let uploads = Arc::new(Mutex::new(
            uploads::Manager::new(dir.join("uploads")).expect("uploads"),
        ));
        AppState::new(
            d.config,
            d.pairing,
            d.workspaces,
            d.events,
            d.hub,
            d.acp,
            d.permissions,
            None,
            Some(uploads),
            None,
        )
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
        let state = test_support::test_state(dir.path());
        (dir, state)
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
            .verify_passcode(&session.passcode, "Device", None)
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
        let error = authorize_request(&state.pairing, "192.168.1.2:9000", &Method::GET, &headers)
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
            let _ = state
                .pairing
                .verify_passcode("invalid", "device", Some("127.0.0.1"));
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

    // --- Browse preview route (`GET /preview/{id}/{*path}`) ---

    /// Registers a temp workspace with a small static site fixture.
    async fn preview_fixture_workspace(
        state: &AppState,
    ) -> (tempfile::TempDir, crate::interfaces::WorkspaceInfo) {
        let site = tempfile::tempdir().expect("preview fixture dir");
        tokio::fs::write(
            site.path().join("index.html"),
            b"<!DOCTYPE html><html><link rel=\"stylesheet\" href=\"./styles.css\"><body>hi</body></html>",
        )
        .await
        .expect("write index.html");
        tokio::fs::write(site.path().join("styles.css"), b"body{color:red}")
            .await
            .expect("write styles.css");
        let info = state
            .workspaces
            .register(site.path().to_str().expect("utf-8 path"))
            .await
            .expect("register workspace");
        (site, info)
    }

    #[tokio::test]
    async fn preview_serves_html_inline_with_content_type() {
        let (_state_dir, state) = state();
        let (_site, ws) = preview_fixture_workspace(&state).await;

        let response = oneshot(
            state,
            Request::builder()
                .uri(format!("/preview/{}/index.html", ws.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let ctype = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(
            ctype.starts_with("text/html"),
            "expected text/html, got {ctype}"
        );
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .and_then(|v| v.to_str().ok()),
            Some("inline")
        );
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .and_then(|v| v.to_str().ok()),
            Some("frame-ancestors 'self'; sandbox allow-same-origin")
        );
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(
            String::from_utf8_lossy(&bytes).contains("styles.css"),
            "body should be the fixture HTML"
        );
    }

    #[tokio::test]
    async fn preview_rejects_path_traversal() {
        let (_state_dir, state) = state();
        let (_site, ws) = preview_fixture_workspace(&state).await;

        // Literal `..` segments (tower/axum may keep these on Request::builder).
        let response = oneshot(
            state.clone(),
            Request::builder()
                .uri(format!("/preview/{}/../../../etc/passwd", ws.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert!(
            response.status() == StatusCode::BAD_REQUEST
                || response.status() == StatusCode::FORBIDDEN
                || response.status() == StatusCode::NOT_FOUND,
            "traversal must be rejected, got {}",
            response.status()
        );

        // Percent-encoded dots — what a non-normalizing client can send; must
        // hit WorkspaceMgr.file_path containment (not SPA).
        let response = oneshot(
            state,
            Request::builder()
                .uri(format!(
                    "/preview/{}/{}etc/passwd",
                    ws.id, "%2e%2e/%2e%2e/%2e%2e/"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert!(
            response.status() == StatusCode::BAD_REQUEST
                || response.status() == StatusCode::FORBIDDEN
                || response.status() == StatusCode::NOT_FOUND,
            "encoded traversal must be rejected, got {}",
            response.status()
        );
    }

    #[tokio::test]
    async fn preview_empty_path_returns_404() {
        let (_state_dir, state) = state();
        let (_site, ws) = preview_fixture_workspace(&state).await;

        let response = oneshot(
            state,
            Request::builder()
                .uri(format!("/preview/{}", ws.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn preview_requires_auth_for_non_loopback() {
        // Non-loopback ConnectInfo peer with no Bearer header and no preview
        // ticket must be rejected. The previewToken cookie flow is covered by
        // `preview_session_cookie_authenticates_relative_asset`.
        let (_state_dir, state) = state();
        let (_site, ws) = preview_fixture_workspace(&state).await;

        let response = oneshot_peer(
            state,
            Request::builder()
                .uri(format!("/preview/{}/index.html", ws.id))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn preview_session_cookie_authenticates_relative_asset() {
        let (_state_dir, state, cred) = pending_actions_state(0, false);
        let (_site, ws) = preview_fixture_workspace(&state).await;
        let session = oneshot_peer(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/preview-session", ws.id))
                .header(header::AUTHORIZATION, bearer(&cred))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(session.status(), StatusCode::OK);
        let token = json_body(session).await["token"]
            .as_str()
            .expect("preview token")
            .to_owned();

        let entry = oneshot_peer(
            state.clone(),
            Request::builder()
                .uri(format!(
                    "/preview/{}/index.html?previewToken={token}",
                    ws.id
                ))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(entry.status(), StatusCode::OK);
        let cookie = entry
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("preview cookie");
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains(&format!("Path=/preview/{}/", ws.id)));
        assert_ne!(
            cookie.split(';').next(),
            Some(format!("preview_token={token}").as_str())
        );

        let asset = oneshot_peer(
            state.clone(),
            Request::builder()
                .uri(format!("/preview/{}/styles.css", ws.id))
                .header(
                    header::COOKIE,
                    cookie.split(';').next().expect("cookie pair"),
                )
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(asset.status(), StatusCode::OK);

        let replay = oneshot_peer(
            state,
            Request::builder()
                .uri(format!(
                    "/preview/{}/index.html?previewToken={token}",
                    ws.id
                ))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn tls_preview_entry_sets_secure_cookie() {
        let (_state_dir, state, cred) = pending_actions_state(0, false);
        let (_site, ws) = preview_fixture_workspace(&state).await;
        let session = oneshot_peer(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/preview-session", ws.id))
                .header(header::AUTHORIZATION, bearer(&cred))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        let token = json_body(session).await["token"]
            .as_str()
            .expect("preview token")
            .to_owned();
        let mut request = Request::builder()
            .uri(format!(
                "/preview/{}/index.html?previewToken={token}",
                ws.id
            ))
            .body(Body::empty())
            .expect("request");
        request.extensions_mut().insert(TlsConnection);
        let entry = oneshot_peer(state, request, "10.0.0.1:9").await;
        let cookie = entry
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("preview cookie");
        assert!(cookie.contains("; Secure"));
    }

    #[tokio::test]
    async fn preview_token_wrong_workspace_is_rejected() {
        let (_state_dir, state, cred) = pending_actions_state(0, false);
        let (_site, first) = preview_fixture_workspace(&state).await;
        let (_other_site, second) = preview_fixture_workspace(&state).await;
        let session = oneshot_peer(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/preview-session", first.id))
                .header(header::AUTHORIZATION, bearer(&cred))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        let session_body = json_body(session).await;
        let token = session_body["token"].as_str().expect("preview token");
        let response = oneshot_peer(
            state,
            Request::builder()
                .uri(format!(
                    "/preview/{}/index.html?previewToken={token}",
                    second.id
                ))
                .body(Body::empty())
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // --- Workspace file mutations (delete / rename / mkdir) ---

    #[tokio::test]
    async fn file_mutations_happy_path_and_errors() {
        let (_state_dir, state) = state();
        let site = tempfile::tempdir().expect("workspace dir");
        tokio::fs::write(site.path().join("a.txt"), b"hello")
            .await
            .expect("write a.txt");
        let ws = state
            .workspaces
            .register(site.path().to_str().expect("utf-8"))
            .await
            .expect("register");

        // mkdir
        let mkdir = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/mkdir", ws.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"path":"src/foo"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(mkdir.status(), StatusCode::OK);
        assert_eq!(json_body(mkdir).await["status"], "created");
        assert!(site.path().join("src/foo").is_dir());

        // rename
        let rename = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/rename", ws.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"from":"a.txt","to":"b.txt"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(rename.status(), StatusCode::OK);
        assert_eq!(json_body(rename).await["status"], "renamed");

        // delete
        let delete = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/workspaces/{}/file?path=b.txt", ws.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(delete.status(), StatusCode::OK);
        assert_eq!(json_body(delete).await["status"], "deleted");

        // delete missing → 404
        let missing = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/workspaces/{}/file?path=gone.txt", ws.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        // traversal → 400
        let traversal = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/mkdir", ws.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"path":"../outside"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(traversal.status(), StatusCode::BAD_REQUEST);
    }

    /// `POST /api/sessions/{id}/profile` returns 400 for a malformed JSON body.
    #[tokio::test]
    async fn session_profile_endpoint_rejects_bad_body() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/sessions/sess-fake/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{not json"#))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// `POST /api/sessions/{id}/profile` returns 400 for an empty profile id.
    #[tokio::test]
    async fn session_profile_endpoint_rejects_empty_profile() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/sessions/sess-fake/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"profile":""}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// `POST /api/sessions/{id}/profile` returns 404 for a missing session.
    #[tokio::test]
    async fn session_profile_endpoint_missing_session_is_not_found() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/sessions/sess-does-not-exist/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"profile":"code"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// `POST /api/sessions/{id}/profile` returns 400 for an unknown profile id.
    #[tokio::test]
    async fn session_profile_endpoint_rejects_unknown_profile() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/sessions/sess-fake/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"profile":"no-such-profile"}"#))
                .expect("request"),
        )
        .await;
        // Unknown profile is rejected before the session lookup, so 400 wins
        // over 404 (validation order: profile id, then session existence).
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// `/api/sessions/{id}/prompt` no longer reads a `profile` body field.
    ///
    /// Sending one is silently ignored by serde (the field is absent from
    /// `PromptRequest`); the prompt still fails on the missing session with
    /// 404, proving the request was parsed without the profile field affecting
    /// behavior. This locks in the S-PROF-ACP wire change.
    #[tokio::test]
    async fn prompt_endpoint_ignores_profile_body_field() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/sessions/sess-does-not-exist/prompt")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"content":"hi","profile":"ask","attachments":[]}"#,
                ))
                .expect("request"),
        )
        .await;
        // 404 (missing session), not 400 — proves the body parsed fine and the
        // profile field was silently dropped by serde.
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
