//! Session lifecycle, prompt, profile, and validation handlers.

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{error, warn};

use crate::interfaces::{
    ACPClient, Attachment, EditedFile, Event, EventStore, EventType, ProfileTransitionPreview,
    ProfileTransitionStrategy, SessionInfo, WorkspaceManager,
};

use super::{app_error, decode_json_body, required_query, ApiResponseError, AppState};

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

/// `GET /api/sessions/{id}/edited-files` — agent-written files still pending
/// review in the in-memory cache, enriched with recent `FileWritten` metadata.
/// Deduplicated by path; the latest write wins.
pub(super) async fn edited_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<EditedFile>>, ApiResponseError> {
    // Validate the session before inspecting cache/event state so callers
    // cannot probe stale cache keys belonging to an unknown session.
    state.acp.get_session_info(&id).map_err(app_error)?;
    let cached = state.edited_files.list(&id);
    if cached.is_empty() {
        return Ok(Json(Vec::new()));
    }

    // Read from the tail: a long conversation can exceed the event cap, and
    // the metadata relevant to the in-memory cache is necessarily recent.
    let events = state
        .events
        .store()
        .query_before(&id, 0, crate::api::events::MAX_EVENT_LIMIT)
        .await
        .map_err(app_error)?;

    // Event IDs are monotonic, so the highest ID is the most recent write for
    // a workspace-relative path.
    let mut latest: std::collections::HashMap<String, &Event> = std::collections::HashMap::new();
    for event in &events {
        if event.event_type != EventType::FileWritten {
            continue;
        }
        let path = &event.target;
        if latest
            .get(path)
            .is_none_or(|previous| event.id > previous.id)
        {
            latest.insert(path.clone(), event);
        }
    }

    // The cache is authoritative for pending review. Accept/revert removes an
    // entry, while a later agent write recreates it; durable historical events
    // alone must not resurrect already-resolved edits.
    let mut files: Vec<EditedFile> = cached
        .into_iter()
        .map(|(path, added_lines, removed_lines, recorded_at)| {
            // Do not attach an older accepted write's event to a newer cache
            // entry when publication of the newer write failed.
            let event = latest
                .get(&path)
                .filter(|event| event.timestamp >= recorded_at);
            EditedFile {
                path,
                event_id: event.map_or(0, |event| event.id),
                timestamp: event.map_or(recorded_at, |event| event.timestamp),
                added_lines,
                removed_lines,
            }
        })
        .collect();
    // Stable output makes clients and tests deterministic.
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Json(files))
}

#[derive(Deserialize)]
pub(super) struct EditedFilePathQuery {
    path: Option<String>,
}

/// `GET /api/sessions/{id}/edited-files/diff?path=` — pre-edit vs current
/// content for the agent diff viewer. Returns 404 if no cached pre-edit
/// content exists for this (session, path) pair.
pub(super) async fn edited_file_diff(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<EditedFilePathQuery>,
) -> Result<Json<Value>, ApiResponseError> {
    let path = required_query(query.path, "path")?;
    let session = state.acp.get_session_info(&id).map_err(app_error)?;
    let _write_guard = state.edited_files.lock_writes().await;
    let entry = state
        .edited_files
        .get(&id, &path)
        .ok_or_else(|| ApiResponseError::not_found(format!("no edited-file cache for {path}")))?;
    let current = state
        .workspaces
        .read_file(&session.workspace, &path)
        .await
        .map_err(app_error)?;
    Ok(Json(json!({
        "path": path,
        "base": entry.content_before,
        "head": current.content,
        "truncated": false,
    })))
}

/// `POST /api/sessions/{id}/edited-files/revert?path=` — restore the pre-edit
/// content for a file. Removes the cache entry on success.
pub(super) async fn revert_edited_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<EditedFilePathQuery>,
) -> Result<Json<Value>, ApiResponseError> {
    let path = required_query(query.path, "path")?;
    let session = state.acp.get_session_info(&id).map_err(app_error)?;
    let _write_guard = state.edited_files.lock_writes().await;
    let entry = state
        .edited_files
        .get(&id, &path)
        .ok_or_else(|| ApiResponseError::not_found(format!("no edited-file cache for {path}")))?;
    if entry.existed_before {
        state
            .workspaces
            .write_file(
                &session.workspace,
                &path,
                &entry.content_before,
                entry.written_revision,
            )
            .await
            .map_err(app_error)?;
    } else {
        state
            .workspaces
            .delete_file_if_revision(&session.workspace, &path, entry.written_revision)
            .await
            .map_err(app_error)?;
    }
    let _ = state.edited_files.remove(&id, &path);
    // Revert is an app-originated disk mutation, so fswatch suppresses its
    // echo. Publish the same refresh signal used for ACP writes so other
    // connected clients reload clean editor tabs and the pending cache list.
    let mut event = Event::new(0, EventType::FileWritten, &id, chrono::Utc::now());
    event.workspace_id = session.workspace;
    event.target = path.clone();
    if let Err(error) = state.events.append_and_publish(event).await {
        tracing::error!(
            session_id = %id,
            path,
            %error,
            "failed to publish file refresh after edited-file revert"
        );
    }
    Ok(Json(json!({ "path": path, "reverted": true })))
}

/// `POST /api/sessions/{id}/edited-files/accept?path=` — mark one cached edit
/// as reviewed without changing its on-disk content.
pub(super) async fn accept_edited_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<EditedFilePathQuery>,
) -> Result<Json<Value>, ApiResponseError> {
    let path = required_query(query.path, "path")?;
    state.acp.get_session_info(&id).map_err(app_error)?;
    let _write_guard = state.edited_files.lock_writes().await;
    if !state.edited_files.remove(&id, &path) {
        return Err(ApiResponseError::not_found(format!(
            "no edited-file cache for {path}"
        )));
    }
    Ok(Json(json!({ "path": path, "accepted": true })))
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
        // 0 means "unspecified" here; the ACP layer floors it at the daemon
        // default so an omitted cap never means an unbounded transcript.
        let max_transfer = request.max_transfer_bytes.unwrap_or_default();
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

/// Query for `GET /api/sessions/{id}/profile/preview`.
#[derive(Deserialize)]
pub(super) struct ProfilePreviewQuery {
    /// Profile id to price against the session's current access.
    profile: String,
}

/// `GET /api/sessions/{id}/profile/preview` — would this profile change MCP
/// server access?
///
/// Read-only and side-effect free. The UI calls it before offering a profile
/// switch so it only interrupts the user when applying the profile in place
/// would leave the session's real tool access behind the selector.
pub(super) async fn preview_session_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
    query: Result<Query<ProfilePreviewQuery>, QueryRejection>,
) -> Result<Json<ProfileTransitionPreview>, ApiResponseError> {
    let Query(query) =
        query.map_err(|_| ApiResponseError::bad_request("profile query parameter is required"))?;
    state
        .acp
        .preview_session_profile(&id, &query.profile)
        .await
        .map(Json)
        .map_err(app_error)
}

/// Request body for `POST /api/sessions/{id}/profile/transition`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TransitionProfileRequest {
    /// Target profile id (validated against the loaded config; unknown → 400).
    profile: String,
    /// `history` moves this conversation; `fresh` opens a separate one. An
    /// unrecognized value is rejected by serde rather than defaulted — picking
    /// one would start or replace an agent session the caller did not ask for.
    strategy: ProfileTransitionStrategy,
    /// Optional cap on the transcript carried into the replacement session's
    /// first prompt. Omitted or `<= 0` uses the daemon default.
    max_transfer_bytes: Option<i64>,
}

/// `POST /api/sessions/{id}/profile/transition` — apply a profile whose MCP
/// server set differs, by starting a new agent session.
///
/// Returns the session the client should display: the same one for `history`,
/// a newly created one for `fresh`. Use `POST /api/sessions/{id}/profile`
/// instead when only instructions should change.
pub(super) async fn transition_session_profile(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<TransitionProfileRequest>, JsonRejection>,
) -> Result<Json<SessionInfo>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if request.profile.trim().is_empty() {
        return Err(ApiResponseError::bad_request("profile is required"));
    }
    state
        .acp
        .transition_session_profile(
            &id,
            &request.profile,
            request.strategy,
            request.max_transfer_bytes.unwrap_or_default(),
        )
        .await
        .map(Json)
        .map_err(app_error)
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
    use axum::body::{to_bytes, Body};
    use axum::extract::ConnectInfo;
    use axum::http::{header, Request, StatusCode};
    use serde_json::Value;
    use std::net::SocketAddr;
    use tower::ServiceExt;

    use crate::acp::{ConversationStore, StoredSession};
    use crate::api::{router, test_support, AppState};
    use crate::interfaces::{EditedFile, Event, EventType, SessionInfo, WorkspaceManager};

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

    async fn seed_dormant_session(
        state: &AppState,
        dir: &tempfile::TempDir,
        session_id: &str,
    ) -> String {
        let workspace = state
            .workspaces
            .register(dir.path().to_str().expect("UTF-8 workspace path"))
            .await
            .expect("register workspace");
        let now = chrono::Utc::now();
        ConversationStore::new(Some(dir.path().join("conversations.json")))
            .persist(&[StoredSession::from_parts(
                SessionInfo {
                    id: session_id.to_string(),
                    name: "Edited file test".to_string(),
                    status: "idle".to_string(),
                    agent_id: "mock".to_string(),
                    model_id: "mock-model".to_string(),
                    workspace: workspace.id.clone(),
                    created_at: now,
                    updated_at: now,
                },
                "",
            )])
            .expect("persist session");
        state
            .acp
            .load_conversations()
            .expect("load persisted session");
        workspace.id
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

    #[tokio::test]
    /// The preview is read-only, so a missing session is 404 rather than 400.
    async fn profile_preview_missing_session_is_not_found() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method("GET")
                .uri("/api/sessions/sess-does-not-exist/profile/preview?profile=code")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    /// Without a `profile` query parameter there is nothing to compare against.
    async fn profile_preview_requires_a_profile_query_parameter() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method("GET")
                .uri("/api/sessions/sess-fake/profile/preview")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    /// An unrecognized strategy must be rejected outright — silently picking a
    /// default would start or destroy an agent session the caller did not ask
    /// for.
    async fn profile_transition_rejects_unknown_strategy() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method("POST")
                .uri("/api/sessions/sess-fake/profile/transition")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"profile":"code","strategy":"instructionsOnly"}"#,
                ))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    /// Validation order: the profile id is checked before the session exists,
    /// matching `POST /api/sessions/{id}/profile`.
    async fn profile_transition_rejects_unknown_profile() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method("POST")
                .uri("/api/sessions/sess-fake/profile/transition")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"profile":"nope","strategy":"history"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    /// A valid request against a missing session is 404, not 400.
    async fn profile_transition_missing_session_is_not_found() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method("POST")
                .uri("/api/sessions/sess-does-not-exist/profile/transition")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"profile":"code","strategy":"fresh"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn edited_file_diff_returns_cached_base_and_current_head() {
        let (dir, state) = state();
        let session_id = "session-edited-file-diff";
        let workspace_id = seed_dormant_session(&state, &dir, session_id).await;
        let revision = state
            .workspaces
            .write_file(&workspace_id, "main.rs", "head", 0)
            .await
            .expect("write current content");
        assert!(state.edited_files.record(
            session_id,
            "main.rs",
            "base".to_string(),
            "head",
            true,
            revision,
        ));

        let response = oneshot(
            state,
            Request::builder()
                .uri(format!(
                    "/api/sessions/{session_id}/edited-files/diff?path=main.rs"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let diff: Value = serde_json::from_slice(&body).expect("diff JSON");
        assert_eq!(diff["path"], "main.rs");
        assert_eq!(diff["base"], "base");
        assert_eq!(diff["head"], "head");
        assert_eq!(diff["truncated"], false);
    }

    #[tokio::test]
    async fn revert_edited_file_restores_base_and_removes_cache_entry() {
        let (dir, state) = state();
        let session_id = "session-edited-file-revert";
        let workspace_id = seed_dormant_session(&state, &dir, session_id).await;
        let revision = state
            .workspaces
            .write_file(&workspace_id, "main.rs", "head", 0)
            .await
            .expect("write current content");
        assert!(state.edited_files.record(
            session_id,
            "main.rs",
            "base".to_string(),
            "head",
            true,
            revision,
        ));

        let response = oneshot(
            state.clone(),
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/sessions/{session_id}/edited-files/revert?path=main.rs"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let reverted: Value = serde_json::from_slice(&body).expect("revert JSON");
        assert_eq!(reverted["path"], "main.rs");
        assert_eq!(reverted["reverted"], true);
        assert_eq!(
            state
                .workspaces
                .read_file(&workspace_id, "main.rs")
                .await
                .expect("read reverted content")
                .content,
            "base"
        );
        assert!(state.edited_files.get(session_id, "main.rs").is_none());
    }

    #[tokio::test]
    async fn revert_edited_file_deletes_a_file_created_by_the_agent() {
        let (dir, state) = state();
        let session_id = "session-edited-file-revert-created";
        let workspace_id = seed_dormant_session(&state, &dir, session_id).await;
        let revision = state
            .workspaces
            .write_file(&workspace_id, "created.rs", "agent content", 0)
            .await
            .expect("write agent-created content");
        assert!(state.edited_files.record(
            session_id,
            "created.rs",
            String::new(),
            "agent content",
            false,
            revision,
        ));

        let response = oneshot(
            state.clone(),
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/sessions/{session_id}/edited-files/revert?path=created.rs"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            state
                .workspaces
                .read_file(&workspace_id, "created.rs")
                .await
                .is_err(),
            "reverting a created file must restore its prior nonexistence"
        );
        assert!(state.edited_files.get(session_id, "created.rs").is_none());
    }

    #[tokio::test]
    async fn edited_file_diff_and_revert_return_not_found_without_cache_entry() {
        let (_dir, state) = state();
        let diff = oneshot(
            state.clone(),
            Request::builder()
                .uri("/api/sessions/session-missing/edited-files/diff?path=main.rs")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(diff.status(), StatusCode::NOT_FOUND);

        let revert = oneshot(
            state,
            Request::builder()
                .method("POST")
                .uri("/api/sessions/session-missing/edited-files/revert?path=main.rs")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(revert.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn edited_file_endpoints_reject_cached_path_traversal() {
        let (dir, state) = state();
        let session_id = "session-edited-file-traversal";
        let _workspace_id = seed_dormant_session(&state, &dir, session_id).await;
        // Production cache keys only come from successful workspace writes.
        // Seed a hostile key directly to prove the filesystem boundary remains
        // authoritative even if that invariant is ever broken.
        assert!(state.edited_files.record(
            session_id,
            "../outside.txt",
            "outside base".to_string(),
            "outside head",
            true,
            1,
        ));

        let diff = oneshot(
            state.clone(),
            Request::builder()
                .uri(format!(
                    "/api/sessions/{session_id}/edited-files/diff?path=..%2Foutside.txt"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(diff.status(), StatusCode::BAD_REQUEST);

        let revert = oneshot(
            state,
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/sessions/{session_id}/edited-files/revert?path=..%2Foutside.txt"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(revert.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn revert_rejects_a_newer_file_revision_without_removing_cache() {
        let (dir, state) = state();
        let session_id = "session-edited-file-stale";
        let workspace_id = seed_dormant_session(&state, &dir, session_id).await;
        let agent_revision = state
            .workspaces
            .write_file(&workspace_id, "main.rs", "agent head", 0)
            .await
            .expect("write agent content");
        assert!(state.edited_files.record(
            session_id,
            "main.rs",
            "base".to_string(),
            "agent head",
            true,
            agent_revision,
        ));
        state
            .workspaces
            .write_file(&workspace_id, "main.rs", "newer user edit", agent_revision)
            .await
            .expect("write newer content");

        let response = oneshot(
            state.clone(),
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/sessions/{session_id}/edited-files/revert?path=main.rs"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            state
                .workspaces
                .read_file(&workspace_id, "main.rs")
                .await
                .expect("read current content")
                .content,
            "newer user edit"
        );
        assert!(state.edited_files.get(session_id, "main.rs").is_some());
    }

    #[tokio::test]
    async fn accept_removes_pending_edit_without_changing_the_file() {
        let (dir, state) = state();
        let session_id = "session-edited-file-accept";
        let workspace_id = seed_dormant_session(&state, &dir, session_id).await;
        let revision = state
            .workspaces
            .write_file(&workspace_id, "main.rs", "head", 0)
            .await
            .expect("write current content");
        assert!(state.edited_files.record(
            session_id,
            "main.rs",
            "base".to_string(),
            "head",
            true,
            revision,
        ));

        let response = oneshot(
            state.clone(),
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/sessions/{session_id}/edited-files/accept?path=main.rs"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            state
                .workspaces
                .read_file(&workspace_id, "main.rs")
                .await
                .expect("read accepted content")
                .content,
            "head"
        );
        assert!(state.edited_files.get(session_id, "main.rs").is_none());
    }

    #[tokio::test]
    /// The endpoint retains only the newest `FileWritten` event for each path.
    async fn edited_files_deduplicates_paths_by_latest_event_id() {
        let (dir, state) = state();
        let session_id = "session-edited-files";
        let _workspace_id = seed_dormant_session(&state, &dir, session_id).await;
        assert!(state.edited_files.record(
            session_id,
            "src/main.rs",
            "one\ntwo".to_string(),
            "one\nthree\nfour",
            true,
            1,
        ));
        assert!(state.edited_files.record(
            session_id,
            "README.md",
            String::new(),
            "read me",
            false,
            1,
        ));

        let mut initial_main =
            Event::new(0, EventType::FileWritten, session_id, chrono::Utc::now());
        initial_main.workspace_id = "workspace-1".to_string();
        initial_main.target = "src/main.rs".to_string();
        let initial_main = state
            .events
            .append_and_publish(initial_main)
            .await
            .expect("store initial main write");

        let mut readme = Event::new(0, EventType::FileWritten, session_id, chrono::Utc::now());
        readme.workspace_id = "workspace-1".to_string();
        readme.target = "README.md".to_string();
        let readme = state
            .events
            .append_and_publish(readme)
            .await
            .expect("store README write");

        let mut latest_main = Event::new(0, EventType::FileWritten, session_id, chrono::Utc::now());
        latest_main.workspace_id = "workspace-1".to_string();
        latest_main.target = "src/main.rs".to_string();
        let latest_main = state
            .events
            .append_and_publish(latest_main)
            .await
            .expect("store latest main write");

        let response = oneshot(
            state,
            Request::builder()
                .uri(format!("/api/sessions/{session_id}/edited-files"))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let files: Vec<EditedFile> = serde_json::from_slice(&body).expect("edited files JSON");
        assert_eq!(files.len(), 2);
        assert_eq!(
            files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["README.md", "src/main.rs"]
        );
        assert_eq!(
            files
                .iter()
                .find(|file| file.path == "src/main.rs")
                .expect("main file")
                .event_id,
            latest_main.id
        );
        let main = files
            .iter()
            .find(|file| file.path == "src/main.rs")
            .expect("main file");
        assert_eq!((main.added_lines, main.removed_lines), (2, 1));
        assert_ne!(initial_main.id, latest_main.id);
        assert_eq!(
            files
                .iter()
                .find(|file| file.path == "README.md")
                .expect("README file")
                .event_id,
            readme.id
        );
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
