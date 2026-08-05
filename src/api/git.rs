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

use crate::git::{
    self, commit_diff, detect, CommitDiffResult, DiffResult, GitError, GitRepoInfo, LogResult,
    StatusResult,
};
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
            // GitError → AppError via its From impl.
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
pub(crate) struct CommitDiffQuery {
    oid: Option<String>,
}

/// `GET /api/workspaces/{id}/git/log` query params (S-GIT-LOG-API).
#[derive(Deserialize)]
pub(crate) struct LogQuery {
    /// Max commits to return (clamped to 200 by the backend). Defaults to 100.
    #[serde(default)]
    limit: Option<u32>,
    /// Number of commits to skip (for pagination). Defaults to 0.
    #[serde(default)]
    offset: Option<u32>,
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

#[derive(Deserialize)]
pub(crate) struct FetchPullRequest {
    remote: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CheckoutRequest {
    branch: String,
}

#[derive(Deserialize)]
pub(crate) struct CheckoutCommitRequest {
    oid: String,
}

#[derive(Deserialize)]
pub(crate) struct IgnoreRequest {
    patterns: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct DiscardRequest {
    paths: Vec<String>,
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

/// `GET /api/workspaces/{id}/git/commit-diff?oid=...`.
pub async fn get_git_commit_diff(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<CommitDiffQuery>,
) -> Result<Json<CommitDiffResult>, ApiResponseError> {
    let oid = required_query(query.oid, "oid")?;
    let root = workspace_root(&state, &id).await?;
    let result = run_git_blocking("commit diff", move || commit_diff(&root, &oid)).await?;
    Ok(Json(result))
}

/// `GET /api/workspaces/{id}/git/log?limit=...&offset=...` (S-GIT-LOG-API).
///
/// Returns a paginated commit list with parent refs, branch labels, and the
/// HEAD marker. Defaults: `limit=100`, `offset=0`. `limit` is clamped to 200
/// by the backend. An unborn repo returns an empty list, not an error.
pub async fn get_git_log(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<LogQuery>,
) -> Result<Json<LogResult>, ApiResponseError> {
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);
    let root = workspace_root(&state, &id).await?;
    let result = run_git_blocking("log", move || git::log(&root, limit, offset)).await?;
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
    // If-Match is optional: a missing precondition is only accepted for the
    // initial commit (unborn HEAD). See `git::commit`.
    let expected_head = headers
        .get("if-match")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(std::string::ToString::to_string);
    let root = workspace_root(&state, &id).await?;
    let oid = tokio::task::spawn_blocking(move || {
        git::commit(
            &root,
            &request.message,
            expected_head.as_deref(),
            request.amend,
        )
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

/// `POST /api/workspaces/{id}/git/fetch`.
pub async fn fetch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<FetchPullRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    let root = workspace_root(&state, &id).await?;
    let output = run_git_blocking("fetch", move || {
        git::fetch(&root, request.remote.as_deref())
    })
    .await?;
    Ok(Json(json!({ "ok": true, "stderr": output })))
}

/// `POST /api/workspaces/{id}/git/pull`. Refuses with 409 when the working
/// tree is dirty — the `GitError::DirtyTree` → `AppError::conflict` mapping
/// in `From<GitError> for AppError` handles the status code.
pub async fn pull(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<FetchPullRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    let root = workspace_root(&state, &id).await?;
    let output =
        run_git_blocking("pull", move || git::pull(&root, request.remote.as_deref())).await?;
    Ok(Json(json!({ "ok": true, "stderr": output })))
}

/// `POST /api/workspaces/{id}/git/checkout`. Refuses with 409 when the working
/// tree is dirty — same `GitError::DirtyTree` → `AppError::conflict` mapping
/// as pull.
pub async fn checkout(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<CheckoutRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if request.branch.is_empty() {
        return Err(ApiResponseError::bad_request("branch required"));
    }
    let root = workspace_root(&state, &id).await?;
    let output =
        run_git_blocking("checkout", move || git::checkout(&root, &request.branch)).await?;
    Ok(Json(json!({ "ok": true, "stderr": output })))
}

/// `POST /api/workspaces/{id}/git/checkout-commit`. Checks out a commit SHA
/// into detached HEAD. Same 409 dirty-tree guard as branch checkout.
pub async fn checkout_commit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<CheckoutCommitRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if request.oid.is_empty() {
        return Err(ApiResponseError::bad_request("oid required"));
    }
    let root = workspace_root(&state, &id).await?;
    let output = run_git_blocking("checkout-commit", move || {
        git::checkout_commit(&root, &request.oid)
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

/// `POST /api/workspaces/{id}/git/ignore` — append patterns to `.gitignore`.
///
/// Named `ignore_paths` (not `add_to_gitignore`) to avoid clashing with the
/// `git::add_to_gitignore` function. The op does not require a repo, but it
/// still runs on a blocking thread because file I/O is synchronous.
pub async fn ignore_paths(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<IgnoreRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if request.patterns.is_empty() {
        return Err(ApiResponseError::bad_request("patterns required"));
    }
    let root = workspace_root(&state, &id).await?;
    let added = run_git_blocking("ignore", move || {
        git::add_to_gitignore(&root, &request.patterns)
    })
    .await?;
    Ok(Json(json!({ "added": added })))
}

/// `POST /api/workspaces/{id}/git/discard` — restore tracked files to their
/// index state and delete untracked files. Same trust model as stage/unstage
/// (paired device or loopback, workspace-scoped path containment).
pub async fn discard(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<DiscardRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if request.paths.is_empty() {
        return Err(ApiResponseError::bad_request("paths required"));
    }
    let root = workspace_root(&state, &id).await?;
    let discarded =
        run_git_blocking("discard", move || git::discard(&root, &request.paths)).await?;
    Ok(Json(json!({ "discarded": discarded })))
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
