use crate::interfaces::AppError;
use thiserror::Error;

/// Read-only snapshot of a workspace's git repo state (S-GIT-DETECT).
///
/// Returned by [`crate::git::detect`] and surfaced verbatim by
/// `GET /api/workspaces/{id}/git`. When `repo_detected` is `false`, every
/// other field is `None`/default and the caller must not assume a repo.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitRepoInfo {
    /// `false` when the workspace root has no `.git` (or it could not be
    /// opened read-only). All other fields are `None` in that case.
    pub repo_detected: bool,
    /// Current branch name, or `None` for a detached HEAD / unborn branch.
    pub head_branch: Option<String>,
    /// Hex object id of the HEAD commit, or `None` for an unborn repo.
    pub head_oid: Option<String>,
    /// True for a `.git/shallow` clone. Surfaced so the UI can warn that
    /// history is partial.
    pub is_shallow: bool,
    /// Best-effort "are there any uncommitted changes" flag, computed from
    /// the index vs. worktree without running a full status. Used by the
    /// action bar badge; full status lives in [`crate::git::status`] (S-GIT-API).
    pub has_uncommitted_changes: bool,
}

/// One row of `GET /api/workspaces/{id}/git/status` (S-GIT-API).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileStatus {
    /// Workspace-relative path with forward slashes.
    pub path: String,
    /// For renames, the previous path; otherwise `None`.
    pub old_path: Option<String>,
    /// `added` | `modified` | `deleted` | `renamed` | `untracked` | `conflicted`.
    pub status: String,
    /// `true` when the change is staged in the index (vs. only in the worktree).
    pub staged: bool,
}

/// `GET /api/workspaces/{id}/git/status` response (S-GIT-API).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatusResult {
    pub head_branch: Option<String>,
    pub head_oid: Option<String>,
    /// Upstream tracking branch name, e.g. `origin/main`, if configured.
    pub upstream: Option<String>,
    /// Commits on HEAD not on upstream.
    pub ahead: u64,
    /// Commits on upstream not on HEAD.
    pub behind: u64,
    pub files: Vec<FileStatus>,
}

/// `GET /api/workspaces/{id}/git/diff` response (S-GIT-API).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    /// Unified-diff text (bounded by [`crate::git::MAX_DIFF_BYTES`]).
    pub unified: String,
    /// Base (old) file content used by the editor merge viewer.
    pub base: String,
    /// Head (new) file content used by the editor merge viewer.
    pub head: String,
    /// `true` when the diff was capped at [`crate::git::MAX_DIFF_BYTES`].
    pub truncated: bool,
}

/// Author identity for a commit log entry (S-GIT-LOG-API).
///
/// `time` is an RFC 3339 / ISO 8601 UTC string so the frontend can format it
/// with `new Date()` directly.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CommitAuthor {
    pub name: String,
    pub email: String,
    pub time: String,
}

/// One row of `GET /api/workspaces/{id}/git/log` (S-GIT-LOG-API).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogCommit {
    /// Hex object id of the commit.
    pub oid: String,
    /// Hex object ids of the parent commits (first-parent order).
    pub parents: Vec<String>,
    /// Commit subject (first line of the message).
    pub message: String,
    /// Author identity + timestamp.
    pub author: CommitAuthor,
    /// Short names of local branches pointing at this commit (e.g. `main`).
    pub branch_labels: Vec<String>,
    /// `true` when this is the commit HEAD points at.
    pub is_head: bool,
}

/// `GET /api/workspaces/{id}/git/log` response (S-GIT-LOG-API).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogResult {
    pub commits: Vec<LogCommit>,
    /// Total reachable commits from HEAD (for pagination UI).
    pub total: u64,
    /// `true` when `offset + commits.len() < total`.
    pub has_more: bool,
}

/// Typed errors for git operations. Mapped to [`AppError`] by callers so the
/// API layer surfaces stable HTTP status codes without re-parsing strings.
#[derive(Debug, Error)]
pub enum GitError {
    /// No `.git` at the workspace root (operation requires a repo).
    #[error("not a git repository")]
    NotARepo,
    /// The `.git` entry is a symlink — rejected per the workspace symlink policy.
    #[error(".git is a symlink; refusing to open")]
    SymlinkedGitDir,
    /// `gix` returned an error opening or reading the repo. Boxed to keep
    /// `GitError` small (the underlying error type is ~128 bytes).
    #[error("git open error: {0}")]
    Open(Box<gix::open::Error>),
    /// A `gix` operation failed (status, diff, stage, commit).
    #[error("git operation failed: {0}")]
    Operation(String),
    /// A path passed in for stage/unstage escaped the workspace root.
    #[error("path escapes workspace root: {0}")]
    PathEscapes(String),
    /// Spawning `git push` failed, or it exited non-zero.
    #[error("git push failed: {0}")]
    Push(String),
    /// Working tree has uncommitted changes; pull/checkout refused.
    #[error("working tree is dirty: {0}")]
    DirtyTree(String),
}

impl From<GitError> for AppError {
    fn from(error: GitError) -> Self {
        match error {
            GitError::NotARepo => AppError::not_found_kind("git repository"),
            GitError::SymlinkedGitDir | GitError::PathEscapes(_) => {
                AppError::validation(error.to_string())
            }
            GitError::DirtyTree(_) => AppError::conflict(error.to_string()),
            GitError::Open(_) | GitError::Operation(_) | GitError::Push(_) => {
                AppError::internal(error.to_string())
            }
        }
    }
}
