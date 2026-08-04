//! HTTP API composition, shared handler plumbing, and router-level checks.
//!
//! This module owns the application state, shared error/body helpers, and route
//! composition. Endpoint behavior lives in the focused child modules, while
//! security policy remains at the edge: pairing is the only unauthenticated
//! API, loopback requests bypass device credentials with an Origin check on
//! mutations, and the WebSocket hub performs its own browser-specific
//! credential and Origin gate.

mod agents;
mod auth;
mod devices;
mod embed;
mod events;
mod files;
mod git;
mod mcp;
mod pair;
mod permissions;
mod preview;
mod profiles;
mod providers;
mod session_extra;
mod sessions;
mod settings;
#[cfg(test)]
pub(crate) mod test_support;
mod workspaces;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::DefaultBodyLimit;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{error, warn};

use crate::acp::Client;
use crate::config::ConfigStore;
use crate::events::SharedEventBus;
use crate::interfaces::{map_api_error, AppError, Event, EventType};
use crate::pairing::{Manager as PairingManager, PairingError};
use crate::permissions::Manager as PermissionsManager;
use crate::sync::Hub;
use crate::uploads;
use crate::workspace::Manager as WorkspaceManagerImpl;

/// JSON body size limit for API requests other than file writes and uploads.
/// Caps malformed or abusive LAN requests before parsing (Go `defaultMaxBodyBytes`).
pub const MAX_API_BODY_BYTES: usize = 10 * 1024 * 1024;

/// File-write exception matching Go `fileWriteMaxBodyBytes` (large editor saves).
pub const FILE_WRITE_MAX_BODY_BYTES: usize = 50 * 1024 * 1024;

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
    pair_rate: Arc<Mutex<HashMap<String, pair::PairRateBucket>>>,
    /// In-memory, workspace-scoped preview tickets with short expiry.
    preview_tokens: Arc<Mutex<HashMap<String, preview::PreviewToken>>>,
    /// Cached autodetect results with a cooldown to prevent probe-spawn `DoS`.
    autodetect_cache: agents::AutodetectCache,
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
// Single linear request handler — splitting would obscure the flow.
#[allow(clippy::too_many_lines)]
// Keep the composed state passed by value to Axum's router builder.
#[allow(clippy::needless_pass_by_value)]
pub fn router(state: AppState) -> Router {
    let auth_state = state.clone();
    let protected: Router = Router::new()
        .route("/api/devices", get(devices::list_devices))
        .route("/api/devices/{id}", delete(devices::revoke_device))
        .route(
            "/api/devices/cancel-revocation",
            post(devices::cancel_revocation),
        )
        .route("/api/pending-actions", get(devices::list_pending_actions))
        .route(
            "/api/workspaces",
            get(workspaces::list_workspaces).post(workspaces::register_workspace),
        )
        .route("/api/workspaces/{id}", delete(workspaces::remove_workspace))
        .route(
            "/api/workspaces/{id}/trust",
            put(workspaces::set_workspace_trust),
        )
        .route(
            "/api/workspaces/cancel-registration",
            post(workspaces::cancel_workspace_registration),
        )
        .route("/api/workspaces/{id}/files", get(files::file_tree))
        // POST write lives on a sibling router with a 50 MiB body cap (below).
        .route(
            "/api/workspaces/{id}/file",
            get(files::read_file).delete(files::delete_file),
        )
        .route("/api/workspaces/{id}/raw", get(files::raw_file))
        .route(
            "/api/workspaces/{id}/preview-session",
            post(preview::create_preview_session),
        )
        // Browse-preview virtual root: path-based so relative asset URLs resolve.
        // Bare `/preview/{id}` (no file) returns 404 — no SPA fallback.
        .route("/preview/{id}", get(preview::preview_empty))
        .route("/preview/{id}/{*path}", get(preview::preview_file))
        .route("/api/workspaces/{id}/search", get(files::search))
        .route("/api/workspaces/{id}/rename", post(files::rename_path))
        .route("/api/workspaces/{id}/mkdir", post(files::mkdir))
        .route("/api/workspaces/{id}/git", get(git::get_git_state))
        .route("/api/workspaces/{id}/git/status", get(git::get_git_status))
        .route("/api/workspaces/{id}/git/diff", get(git::get_git_diff))
        .route("/api/workspaces/{id}/git/log", get(git::get_git_log))
        .route("/api/workspaces/{id}/git/stage", post(git::stage))
        .route("/api/workspaces/{id}/git/unstage", post(git::unstage))
        .route("/api/workspaces/{id}/git/commit", post(git::commit))
        .route("/api/workspaces/{id}/git/push", post(git::push))
        .route("/api/workspaces/{id}/git/fetch", post(git::fetch))
        .route("/api/workspaces/{id}/git/pull", post(git::pull))
        .route("/api/workspaces/{id}/git/init", post(git::init_repo))
        .route("/api/workspaces/{id}/git/ignore", post(git::ignore_paths))
        .route("/api/workspaces/{id}/git/discard", post(git::discard))
        .route("/api/events", get(events::events))
        .route("/api/events/{session_id}", get(events::session_events))
        .route(
            "/api/agents",
            get(agents::list_agents).post(agents::upsert_agent),
        )
        .route("/api/agents/{id}", delete(agents::delete_agent))
        .route("/api/agents/autodetect", post(agents::autodetect_agents))
        .route(
            "/api/sessions",
            get(sessions::list_sessions).post(sessions::create_session),
        )
        .route(
            "/api/sessions/{id}",
            get(sessions::get_session)
                .patch(sessions::patch_session)
                .delete(sessions::close_session),
        )
        .route("/api/sessions/{id}/prompt", post(sessions::send_prompt))
        .route("/api/sessions/{id}/cancel", post(sessions::cancel_session))
        .route(
            "/api/sessions/{id}/profile",
            post(sessions::set_session_profile),
        )
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
        .route("/api/settings/server", get(settings::get_server_settings))
        .route(
            "/api/permissions/pending",
            get(permissions::pending_permissions),
        )
        .route(
            "/api/permissions/{id}/respond",
            post(permissions::respond_permission),
        )
        .route_layer(middleware::from_fn_with_state(
            auth_state.clone(),
            auth::require_auth,
        ))
        .layer(DefaultBodyLimit::max(MAX_API_BODY_BYTES))
        .with_state(state.clone());

    // Large editor saves need Go's 50 MiB write exception; a global 10 MiB
    // DefaultBodyLimit cannot be raised per-route once applied as an outer
    // layer, so write POST is a sibling authenticated router.
    let file_write: Router = Router::new()
        .route("/api/workspaces/{id}/file", post(files::write_file))
        .route_layer(middleware::from_fn_with_state(
            auth_state,
            auth::require_auth,
        ))
        .layer(DefaultBodyLimit::max(FILE_WRITE_MAX_BODY_BYTES))
        .with_state(state.clone());

    // The hub owns its own handshake authorization because WebSocket
    // credentials are browser query parameters rather than Authorization.
    state
        .hub
        .set_auth_checker(auth::pairing_auth_checker(&state.pairing));

    let pairing_public: Router = Router::new()
        .route("/api/pair/initiate", post(pair::pair_initiate))
        .route(
            "/api/pair/verify-passcode",
            post(pair::pair_verify_passcode),
        )
        .route("/api/pair/verify-token", post(pair::pair_verify_token))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            pair::require_pair_rate_limit,
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
        .fallback(get(embed::spa_fallback))
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
/// allow-scripts` for agent-written HTML). `style-src 'unsafe-inline'` is
/// required because `CodeMirror` injects inline styles; no `'unsafe-inline'`
/// is granted to `script-src`.
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
    // (e.g. the preview endpoint sets `sandbox allow-scripts`).
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

/// `GET /health` is intentionally unauthenticated for host/service probes.
async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CancelActionRequest {
    pub(super) action_id: String,
}

pub(crate) fn required_query(
    value: Option<String>,
    name: &str,
) -> Result<String, ApiResponseError> {
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
pub(super) fn revocation_grace_period(state: &AppState) -> Duration {
    let secs = state.config.read().revocation_grace_period_seconds.max(0);
    Duration::from_secs(u64::try_from(secs).unwrap_or(0))
}

/// Shared cancel flow for grace-period pending actions (cancel → event). The
/// body is decoded by the caller so handlers can perform authorization checks
/// against the action id before cancelling.
pub(super) async fn cancel_pending_action(
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
pub(super) async fn record_event(state: &AppState, event: Event) {
    if let Err(error) = state.events.append_and_publish(event).await {
        warn!(%error, "failed to record pending-action event");
    }
}

/// Cancel handlers always return 404 on pairing miss/type mismatch (Go parity).
pub(super) fn cancel_pending_error(error: PairingError) -> ApiResponseError {
    match error {
        PairingError::PendingActionNotFound | PairingError::PendingActionTypeMismatch => {
            ApiResponseError::not_found(error.to_string())
        }
        other => pairing_error(other),
    }
}

// Call sites in src/api/session_extra.rs and src/api/providers.rs — would require cross-file signature change.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn pairing_error(error: PairingError) -> ApiResponseError {
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

// Call sites in src/api/session_extra.rs and src/api/providers.rs — would require cross-file signature change.
#[allow(clippy::needless_pass_by_value)]
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
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> Self {
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

#[cfg(test)]
mod tests {
    use crate::api::test_support::{json_body, oneshot, pending_actions_state, state};
    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};

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

    // These checks exercise device handlers through the composed router.
    #[tokio::test]
    async fn revoke_device_grace_period_returns_accepted_and_lists() {
        let (_dir, state, credential) = pending_actions_state(300, false);
        let response = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/devices/{}", credential.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let info = json_body(response).await;
        assert_eq!(info["deviceId"], credential.id);
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
        let (_dir, state, credential) = pending_actions_state(0, false);
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/devices/{}", credential.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await["status"], "revoked");
    }

    #[tokio::test]
    async fn cancel_revocation_removes_pending_action() {
        let (_dir, state, credential) = pending_actions_state(300, false);
        let revoke = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/devices/{}", credential.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(revoke.status(), StatusCode::ACCEPTED);
        let action_id = json_body(revoke).await["id"]
            .as_str()
            .expect("action id")
            .to_owned();

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
        assert_eq!(json_body(list).await.as_array().map_or(1, Vec::len), 0);
    }

    #[tokio::test]
    async fn cancel_revocation_not_found_returns_404() {
        let (_dir, state, _credential) = pending_actions_state(300, false);
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
        let (_dir, state, _credential) = pending_actions_state(300, false);
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
        assert_eq!(json_body(response).await["error"], "invalid request body");
    }

    #[tokio::test]
    async fn list_pending_actions_empty_ok() {
        let (_dir, state, _credential) = pending_actions_state(300, false);
        let response = oneshot(
            state,
            Request::builder()
                .uri("/api/pending-actions")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await.as_array().map_or(1, Vec::len), 0);
    }
}
