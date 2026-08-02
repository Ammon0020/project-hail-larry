//! Workspace file tree, content, mutation, search, and raw-file handlers.

use std::path::Path;

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderValue};
use axum::response::Response;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::interfaces::{SearchOptions, WorkspaceManager};

use super::{app_error, decode_json_body, required_query, ApiResponseError, AppState};

pub(super) async fn file_tree(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Vec<crate::interfaces::FileNode>>, ApiResponseError> {
    state
        .workspaces
        .file_tree(&id)
        .await
        .map(Json)
        .map_err(app_error)
}

#[derive(Deserialize)]
pub(super) struct FileQuery {
    path: Option<String>,
}

pub(super) async fn read_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
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

pub(super) async fn raw_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<FileQuery>,
) -> Result<Response, ApiResponseError> {
    let path = required_query(query.path, "path")?;
    serve_workspace_file(&state, &id, &path).await
}

/// Shared body for `/raw` and `/preview`: resolve via workspace containment,
/// read bytes, set Content-Type + inline disposition.
pub(super) async fn serve_workspace_file(
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
    let content_type = content_type_for_path(Path::new(&absolute), &data);
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
    // without this, a workspace `evil.html` opened via `/raw` could call
    // authenticated `/api/*` endpoints and read IDE cookies/localStorage.
    // `sandbox` without `allow-scripts` blocks script execution; the
    // `allow-same-origin` flag is retained so relative subresource loading for
    // legitimate static previews still resolves against the IDE origin. The
    // `/preview` handler overrides this CSP with `frame-ancestors 'self';
    // sandbox allow-scripts` (opaque origin, scripts permitted) because preview
    // is always loaded inside a frontend iframe whose own `sandbox` attribute
    // is the primary script authority; `/raw` is direct-access only and keeps
    // this stricter CSP.
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
fn content_type_for_path(path: &Path, data: &[u8]) -> String {
    let ext = path
        .extension()
        .and_then(|extension| extension.to_str())
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
        _ => infer::get(data).map_or_else(
            || "application/octet-stream".into(),
            |kind| kind.mime_type().to_string(),
        ),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WriteFileRequest {
    path: String,
    content: String,
    #[serde(default)]
    expected_revision: i64,
}

pub(super) async fn write_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
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
pub(super) async fn delete_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
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
pub(super) struct RenamePathRequest {
    from: String,
    to: String,
}

/// `POST /api/workspaces/{id}/rename` — rename/move within the workspace.
/// Overwrite is rejected with 409 when the destination already exists.
pub(super) async fn rename_path(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
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
pub(super) struct MkdirRequest {
    path: String,
}

/// `POST /api/workspaces/{id}/mkdir` — create a directory (parents as needed).
/// Idempotent if the path already exists as a directory; 409 if it is a file.
pub(super) async fn mkdir(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
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
pub(super) struct SearchQuery {
    pattern: Option<String>,
    ignore_case: Option<bool>,
    max_results: Option<i32>,
    file_pattern: Option<String>,
    context_lines: Option<i32>,
}

pub(super) async fn search(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};

    use crate::api::test_support::{json_body, oneshot, state};
    use crate::interfaces::WorkspaceManager;

    #[tokio::test]
    async fn file_mutations_happy_path_and_errors() {
        let (_state_dir, state) = state();
        let workspace_dir = tempfile::tempdir().expect("workspace directory");
        tokio::fs::write(workspace_dir.path().join("a.txt"), b"hello")
            .await
            .expect("write a.txt");
        let workspace = state
            .workspaces
            .register(workspace_dir.path().to_str().expect("UTF-8 path"))
            .await
            .expect("register workspace");

        let mkdir = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/mkdir", workspace.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"path":"src/foo"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(mkdir.status(), StatusCode::OK);
        assert_eq!(json_body(mkdir).await["status"], "created");
        assert!(workspace_dir.path().join("src/foo").is_dir());

        let rename = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/rename", workspace.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"from":"a.txt","to":"b.txt"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(rename.status(), StatusCode::OK);
        assert_eq!(json_body(rename).await["status"], "renamed");

        let delete = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/workspaces/{}/file?path=b.txt", workspace.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(delete.status(), StatusCode::OK);
        assert_eq!(json_body(delete).await["status"], "deleted");

        let missing = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::DELETE)
                .uri(format!(
                    "/api/workspaces/{}/file?path=gone.txt",
                    workspace.id
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let traversal = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/workspaces/{}/mkdir", workspace.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"path":"../outside"}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(traversal.status(), StatusCode::BAD_REQUEST);
    }
}
