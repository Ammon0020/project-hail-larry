//! Session export, open-files context, and image upload handlers.

use std::io::Cursor;
use std::path::Path;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Multipart, Path as AxumPath, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{error, warn};

use crate::acp::{self, EditorSelection};
use crate::interfaces::ACPClient;
use crate::uploads::{self, UploadError, MAX_UPLOAD_BYTES};

use super::{app_error, decode_json_body, ApiResponseError, AppState};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionContextRequest {
    open_files: Option<Vec<String>>,
    recent_edits: Option<Vec<String>>,
    selection: Option<SelectionBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionBody {
    path: String,
    start_line: usize,
    end_line: usize,
    text: String,
}

/// `GET /api/sessions/{id}/export` — markdown transcript attachment.
pub async fn export_session(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Response, ApiResponseError> {
    let info = state.acp.get_session_info(&session_id).map_err(app_error)?;

    let markdown = acp::export_conversation(&state.events, &session_id, 0)
        .await
        .map_err(app_error)?;

    let mut name = info.name;
    if name.is_empty() {
        name = format!("Session {}", short_session_id(&info.id));
    }
    let mut filename = sanitize_download_filename(&name);
    if filename.is_empty() {
        filename = session_id.clone();
    }
    if filename.is_empty() {
        filename = "session".into();
    }
    filename.push_str(".md");

    let disposition = format!("attachment; filename=\"{filename}\"");
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/markdown; charset=utf-8"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&disposition).unwrap_or_else(|_| {
                    HeaderValue::from_static("attachment; filename=\"session.md\"")
                }),
            ),
        ],
        markdown,
    )
        .into_response())
}

/// `GET /api/sessions/{id}/capabilities` — live initialize session-history caps.
///
/// Auth-gated via the shared API router. Does not cold-start agents (Q8 open).
pub async fn session_capabilities(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<crate::interfaces::SessionHistoryCapabilities>, ApiResponseError> {
    state
        .acp
        .session_history_capabilities(&session_id)
        .map(Json)
        .map_err(app_error)
}

/// `POST /api/sessions/{id}/context` — update open-files / selection tracker.
pub async fn session_context(
    State(state): State<AppState>,
    AxumPath(_session_id): AxumPath<String>,
    body: Result<Json<SessionContextRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    let tracker = state.acp.open_files_tracker();
    if let Some(open_files) = request.open_files {
        tracker.set_open_files(open_files).map_err(app_error)?;
    }
    if let Some(recent_edits) = request.recent_edits {
        tracker.set_recent_edits(recent_edits).map_err(app_error)?;
    }
    if let Some(selection) = request.selection {
        tracker
            .set_selection(EditorSelection {
                path: selection.path,
                start_line: selection.start_line,
                end_line: selection.end_line,
                text: selection.text,
            })
            .map_err(app_error)?;
    }
    Ok(Json(json!({"status": "updated"})))
}

/// `POST /api/sessions/{id}/uploads` — multipart image upload.
pub async fn upload_file(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ApiResponseError> {
    let uploads = require_uploads(&state)?;
    if !is_valid_session_id(&session_id) {
        return Err(ApiResponseError::bad_request("invalid session id"));
    }

    let mut file_name = None;
    let mut file_data = None;
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        warn!(%error, "invalid multipart form");
        ApiResponseError::bad_request("invalid multipart form")
    })? {
        if field.name() == Some("file") {
            file_name = Some(field.file_name().unwrap_or("upload").to_string());
            file_data = Some(field.bytes().await.map_err(|error| {
                warn!(%error, "failed to read upload body");
                ApiResponseError::bad_request("invalid multipart form")
            })?);
            break;
        }
    }
    let filename = file_name
        .ok_or_else(|| ApiResponseError::bad_request("missing 'file' field in multipart form"))?;
    let data = file_data
        .ok_or_else(|| ApiResponseError::bad_request("missing 'file' field in multipart form"))?;
    if data.len() > MAX_UPLOAD_BYTES {
        return Err(ApiResponseError::bad_request(format!(
            "failed to store upload: file exceeds {MAX_UPLOAD_BYTES} bytes"
        )));
    }

    let stored = {
        let mut manager = uploads.lock().map_err(|_| {
            error!("uploads manager lock poisoned");
            ApiResponseError::internal("uploads manager unavailable")
        })?;
        manager
            .store(&session_id, &filename, &mut Cursor::new(data.as_ref()))
            .map_err(upload_store_error)?
    };

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": stored.id,
            "name": stored.name,
            "mimeType": stored.mime_type,
            "url": format!("/api/sessions/{session_id}/uploads/{}", stored.id),
            "size": stored.size,
        })),
    ))
}

/// `GET /api/sessions/{id}/uploads/{upload_id}` — serve a stored image.
pub async fn serve_upload(
    State(state): State<AppState>,
    AxumPath((session_id, upload_id)): AxumPath<(String, String)>,
) -> Result<Response, ApiResponseError> {
    let uploads = require_uploads(&state)?;
    if !is_valid_session_id(&session_id) {
        return Err(ApiResponseError::bad_request("invalid session id"));
    }

    let path = {
        let manager = uploads.lock().map_err(|_| {
            error!("uploads manager lock poisoned");
            ApiResponseError::internal("uploads manager unavailable")
        })?;
        manager
            .get(&session_id, &upload_id)
            .map_err(|error| match error {
                UploadError::NotFound { .. } | UploadError::InvalidUploadId => {
                    ApiResponseError::not_found("upload not found")
                }
                other => {
                    error!(%other, "resolve upload failed");
                    ApiResponseError::internal(format!("resolve upload: {other}"))
                }
            })?
    };

    let bytes = tokio::fs::read(&path).await.map_err(|error| {
        error!(path = %path.display(), %error, "read upload failed");
        ApiResponseError::internal(format!("read upload: {error}"))
    })?;

    // The on-disk name is `<upload_id>.<ext>` with a hex id and a magic-byte
    // extension, but sanitize anyway in case the upload-id validation is ever
    // relaxed. Force a download (no inline rendering / MIME sniffing) and keep
    // deleted uploads out of the browser cache.
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mut filename = sanitize_download_filename(&upload_id);
    if filename.is_empty() {
        filename = "upload".into();
    }
    if !ext.is_empty() {
        filename.push('.');
        filename.push_str(ext);
    }
    let disposition = format!("attachment; filename=\"{filename}\"");
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(mime_from_path(&path)),
            ),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&disposition).unwrap_or_else(|_| {
                    HeaderValue::from_static("attachment; filename=\"upload\"")
                }),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        bytes,
    )
        .into_response())
}

fn require_uploads(
    state: &AppState,
) -> Result<&std::sync::Arc<std::sync::Mutex<uploads::Manager>>, ApiResponseError> {
    state
        .uploads
        .as_ref()
        .ok_or_else(|| ApiResponseError::service_unavailable("uploads not configured"))
}

fn upload_store_error(error: UploadError) -> ApiResponseError {
    match error {
        UploadError::InvalidSessionId | UploadError::EmptySessionId => {
            ApiResponseError::bad_request("invalid session id")
        }
        UploadError::Oversize(_)
        | UploadError::UnsupportedType
        | UploadError::InvalidUploadId
        | UploadError::Io(_)
        | UploadError::NotFound { .. } => {
            warn!(%error, "failed to store upload");
            ApiResponseError::bad_request("failed to store upload")
        }
    }
}

/// Reject path-traversal session IDs before touching the uploads root.
fn is_valid_session_id(id: &str) -> bool {
    if id.is_empty() || id == "." || id == ".." || id.contains("..") {
        return false;
    }
    id.bytes()
        .all(|byte| byte != b'/' && byte != b'\\' && byte >= 0x20)
}

fn mime_from_path(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Collapse a session name into a safe download filename slug (Go sanitizeFilename).
pub(crate) fn sanitize_download_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => out.push(ch),
            _ => out.push('_'),
        }
    }
    out.trim_matches('_').to_string()
}

fn short_session_id(id: &str) -> &str {
    const MAX_LEN: usize = 8;
    if id.len() <= MAX_LEN {
        id
    } else {
        &id[..MAX_LEN]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn state_with_uploads(dir: &tempfile::TempDir) -> AppState {
        crate::api::test_support::test_state_with_uploads(dir.path())
    }

    #[test]
    fn sanitize_download_filename_strips_unsafe_runes() {
        assert_eq!(sanitize_download_filename("Hello World!"), "Hello_World");
        assert_eq!(sanitize_download_filename("../x"), "x");
        assert_eq!(sanitize_download_filename("!!!"), "");
    }

    #[tokio::test]
    async fn export_missing_session_is_404() {
        let dir = tempfile::tempdir().expect("temp");
        let response = crate::api::router(state_with_uploads(&dir))
            .oneshot(
                Request::builder()
                    .uri("/api/sessions/missing/export")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn session_context_updates_tracker() {
        let dir = tempfile::tempdir().expect("temp");
        let state = state_with_uploads(&dir);
        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions/sess-1/context")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"openFiles":["a.rs"],"recentEdits":["b.rs"],"selection":{"path":"a.rs","startLine":1,"endLine":2,"text":"hi"}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        // Tracker is process-global on the client; verify via a second write that clears text.
        let clear = crate::api::router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions/sess-1/context")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"selection":{"path":"a.rs","startLine":1,"endLine":1,"text":""}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(clear.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn upload_rejects_path_traversal_session_id() {
        let dir = tempfile::tempdir().expect("temp");
        let boundary = "----boundary";
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"x.png\"\r\nContent-Type: image/png\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(&[0x89, b'P', b'N', b'G']);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let response = crate::api::router(state_with_uploads(&dir))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions/evil..path/uploads")
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
