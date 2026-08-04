//! Session lifecycle, prompt, profile, and validation handlers.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{error, warn};

use crate::interfaces::{ACPClient, Attachment, SessionInfo};

use super::{app_error, decode_json_body, ApiResponseError, AppState};

/// Maximum characters allowed in a session name.
const MAX_SESSION_NAME_CHARS: usize = 128;

/// Validate a client-supplied name before persisting or displaying it.
fn validate_session_name(name: &str) -> Result<(), ApiResponseError> {
    let len = name.chars().count();
    if len > MAX_SESSION_NAME_CHARS {
        return Err(ApiResponseError::bad_request(format!(
            "session name exceeds {MAX_SESSION_NAME_CHARS} characters"
        )));
    }
    if name.chars().any(|character| character < ' ') {
        return Err(ApiResponseError::bad_request(
            "session name contains forbidden control character",
        ));
    }
    Ok(())
}

pub(super) async fn list_sessions(State(state): State<AppState>) -> Json<Vec<SessionInfo>> {
    Json(state.acp.list_sessions())
}

pub(super) async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SessionInfo>, ApiResponseError> {
    state.acp.get_session_info(&id).map(Json).map_err(app_error)
}

// ids are the natural name for these fields.
#[allow(clippy::struct_field_names)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CreateSessionRequest {
    agent_id: String,
    model_id: String,
    workspace_id: String,
    profile_id: Option<String>,
}

pub(super) async fn create_session(
    State(state): State<AppState>,
    body: Result<Json<CreateSessionRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<SessionInfo>), ApiResponseError> {
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
pub(super) struct PatchSessionRequest {
    name: Option<String>,
    agent_id: Option<String>,
    model_id: Option<String>,
    max_transfer_bytes: Option<i64>,
}

pub(super) async fn patch_session(
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
pub(super) struct PromptRequest {
    content: String,
    #[serde(default)]
    attachments: Vec<PromptAttachment>,
}

pub(super) async fn send_prompt(
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

pub(super) async fn cancel_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiResponseError> {
    state.acp.cancel_session(&id).await.map_err(app_error)?;
    Ok(Json(json!({"status": "cancelled"})))
}

/// Request body for `POST /api/sessions/{id}/profile`.
#[derive(Deserialize)]
pub(super) struct SetProfileRequest {
    /// Profile id (validated against the loaded config; unknown → 400).
    profile: String,
}

/// `POST /api/sessions/{id}/profile` — set the active profile for a session.
///
/// Replaces the deprecated `profile` field on `/prompt`. Validates the profile
/// id, stores the selection in the profile middleware, and pushes it to the
/// agent over ACP (`session/set_config_option`, mode category) when the agent
/// advertised the capability. Auth-gated by the protected router.
pub(super) async fn set_session_profile(
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

pub(super) async fn close_session(
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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{header, Request, StatusCode};
    use std::net::SocketAddr;
    use tower::ServiceExt;

    use crate::api::{router, test_support, AppState};

    use super::{validate_session_name, MAX_SESSION_NAME_CHARS};

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("temporary state directory");
        let state = test_support::test_state(dir.path());
        (dir, state)
    }

    async fn oneshot(state: AppState, mut request: Request<Body>) -> axum::response::Response {
        let addr: SocketAddr = "127.0.0.1:9".parse().expect("peer address");
        request.extensions_mut().insert(ConnectInfo(addr));
        router(state).oneshot(request).await.expect("response")
    }

    #[tokio::test]
    /// `POST /api/sessions/{id}/profile` returns 400 for a malformed JSON body.
    async fn session_profile_endpoint_rejects_bad_body() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method("POST")
                .uri("/api/sessions/sess-fake/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r"{not json"))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    /// `POST /api/sessions/{id}/profile` returns 400 for an empty profile id.
    async fn session_profile_endpoint_rejects_empty_profile() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method("POST")
                .uri("/api/sessions/sess-fake/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"profile":""}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    /// `POST /api/sessions/{id}/profile` returns 404 for a missing session.
    async fn session_profile_endpoint_missing_session_is_not_found() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method("POST")
                .uri("/api/sessions/sess-does-not-exist/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"profile":"code"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    /// `POST /api/sessions/{id}/profile` returns 400 for an unknown profile id.
    async fn session_profile_endpoint_rejects_unknown_profile() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method("POST")
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

    #[tokio::test]
    /// `/api/sessions/{id}/prompt` no longer reads a `profile` body field.
    ///
    /// Sending one is silently ignored by serde (the field is absent from
    /// `PromptRequest`); the prompt still fails on the missing session with
    /// 404, proving the request was parsed without the profile field affecting
    /// behavior. This locks in the S-PROF-ACP wire change.
    async fn prompt_endpoint_ignores_profile_body_field() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method("POST")
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

    #[test]
    fn session_name_validation_rejects_oversized_and_control_names() {
        assert_eq!(
            validate_session_name(&"a".repeat(MAX_SESSION_NAME_CHARS + 1))
                .expect_err("oversized name should fail")
                .status,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            validate_session_name("name\nwith-control")
                .expect_err("control character should fail")
                .status,
            StatusCode::BAD_REQUEST
        );
        assert!(validate_session_name("normal session").is_ok());
    }
}
