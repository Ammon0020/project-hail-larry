//! Git REST handlers (`/api/workspaces/{id}/git/...`).
//!
//! Auth is enforced by the shared protected-router middleware (same as the
//! other workspace-scoped endpoints). Path containment is delegated to the
//! workspace manager: every handler resolves the workspace root via
//! [`WorkspaceManager::workspace_root`](crate::interfaces::WorkspaceManager::workspace_root)
//! before calling into [`crate::git`], so the existing workspace symlink/
//! containment policy covers git ops too.
//!
//! # Trust model
//!
//! Caller is any paired device (or loopback). Worst case for the read paths
//! (`GET /git`, `GET /git/status`, `GET /git/diff`) is reading repo state —
//! same sensitivity as `/files`. Write paths (`stage`/`unstage`/`commit`/
//! `push`/`init`) are client-initiated workspace writes. The protected router
//! authenticates paired devices; these operations do not use ACP's
//! session-scoped permission sink.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use tracing::{debug, error};

use crate::git::{self, detect, DiffResult, GitError, GitRepoInfo, StatusResult};
use crate::interfaces::WorkspaceManager;

use super::{app_error, decode_json_body, required_query, ApiResponseError, AppState};

/// `GET /api/workspaces/{id}/git` — read-only repo detection (S-GIT-DETECT).
///
/// Returns `200` with `repo_detected: false` and null fields when the
/// workspace is not a git repo; never 404. The frontend uses this to show
/// the breadcrumb branch and to gate the action bar item.
pub async fn get_git_state(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<GitRepoInfo>, ApiResponseError> {
    let root = state
        .workspaces
        .workspace_root(&id)
        .await
        .map_err(app_error)?;
    // `detect` opens the repo read-only; run it on a blocking thread because
    // gix disk I/O is synchronous and the daemon must not stall the async
    // runtime on a slow/large repo.
    let root_path = PathBuf::from(&root);
    let info = tokio::task::spawn_blocking(move || detect(&root_path))
        .await
        .map_err(|err| {
            error!(%err, "git detect task failed");
            ApiResponseError::internal(format!("git detect task: {err}"))
        })?
        .map_err(|err| {
            debug!(%err, "git detect failed");
            // GitError → AppError via the From impl in src/git/mod.rs.
            app_error(err.into())
        })?;
    Ok(Json(info))
}

#[derive(Deserialize)]
pub(crate) struct DiffQuery {
    path: Option<String>,
    staged: Option<bool>,
}

#[derive(Deserialize)]
pub(crate) struct StageRequest {
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    all: bool,
}

#[derive(Deserialize)]
pub(crate) struct UnstageRequest {
    paths: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct CommitRequest {
    message: String,
    #[serde(default)]
    amend: bool,
}

#[derive(Deserialize)]
pub(crate) struct PushRequest {
    remote: Option<String>,
    #[serde(default)]
    set_upstream: bool,
}

/// `GET /api/workspaces/{id}/git/status`.
pub async fn get_git_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<StatusResult>, ApiResponseError> {
    let root = workspace_root(&state, &id).await?;
    let result = run_git_blocking("status", move || git::status(&root)).await?;
    Ok(Json(result))
}

/// `GET /api/workspaces/{id}/git/diff?path=...&staged=...`.
pub async fn get_git_diff(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<DiffResult>, ApiResponseError> {
    let path = required_query(query.path, "path")?;
    let staged = query.staged.unwrap_or(false);
    let root = workspace_root(&state, &id).await?;
    let result = run_git_blocking("diff", move || git::diff(&root, &path, staged)).await?;
    Ok(Json(result))
}

/// `POST /api/workspaces/{id}/git/stage`.
pub async fn stage(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<StageRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if !request.all && request.paths.is_empty() {
        return Err(ApiResponseError::bad_request("paths or all required"));
    }
    let root = workspace_root(&state, &id).await?;
    let paths = if request.all {
        Vec::new()
    } else {
        request.paths
    };
    let staged = run_git_blocking("stage", move || git::stage(&root, &paths)).await?;
    Ok(Json(json!({ "staged": staged })))
}

/// `POST /api/workspaces/{id}/git/unstage`.
pub async fn unstage(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<UnstageRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    let root = workspace_root(&state, &id).await?;
    let unstaged = run_git_blocking("unstage", move || git::unstage(&root, &request.paths)).await?;
    Ok(Json(json!({ "unstaged": unstaged })))
}

/// `POST /api/workspaces/{id}/git/commit`.
pub async fn commit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<CommitRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    let expected_head = headers
        .get("if-match")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiResponseError::bad_request("If-Match header required"))?
        .to_string();
    let root = workspace_root(&state, &id).await?;
    let oid = tokio::task::spawn_blocking(move || {
        git::commit(&root, &request.message, &expected_head, request.amend)
    })
    .await
    .map_err(|err| blocking_error(&err))?
    .map_err(commit_error)?;
    Ok(Json(json!({ "oid": oid })))
}

/// `POST /api/workspaces/{id}/git/push`.
pub async fn push(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<PushRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    let root = workspace_root(&state, &id).await?;
    let output = run_git_blocking("push", move || {
        git::push(&root, request.remote.as_deref(), request.set_upstream)
    })
    .await?;
    Ok(Json(json!({ "ok": true, "stderr": output })))
}

/// `POST /api/workspaces/{id}/git/init`.
pub async fn init_repo(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let root = workspace_root(&state, &id).await?;
    let oid = run_git_blocking("init", move || git::init(&root)).await?;
    Ok(Json(json!({ "oid": oid })))
}

async fn workspace_root(state: &AppState, id: &str) -> Result<PathBuf, ApiResponseError> {
    state
        .workspaces
        .workspace_root(id)
        .await
        .map(PathBuf::from)
        .map_err(app_error)
}

async fn run_git_blocking<T: Send + 'static>(
    operation: &'static str,
    task: impl FnOnce() -> Result<T, GitError> + Send + 'static,
) -> Result<T, ApiResponseError> {
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|err| blocking_error(&err))?
        .map_err(|err| {
            debug!(%err, ?operation, "git operation failed");
            app_error(err.into())
        })
}

fn blocking_error(err: &tokio::task::JoinError) -> ApiResponseError {
    error!(%err, "git operation task failed");
    ApiResponseError::internal(format!("git operation task: {err}"))
}

fn commit_error(err: GitError) -> ApiResponseError {
    if let GitError::Operation(message) = &err {
        if message.contains("working tree changed") {
            return ApiResponseError::new(StatusCode::CONFLICT, message.clone());
        }
    }
    app_error(err.into())
}
