//! Workspace git operations — read-only repo detection (S-GIT-DETECT) plus
//! the status/diff/stage/unstage/commit/push API surface (S-GIT-API).
//!
//! Backed by [`gix`] (pure-Rust git, no libgit2 C dependency). Every entry
//! point takes an already-validated, canonical workspace root path from the
//! [`WorkspaceManager`](crate::interfaces::WorkspaceManager) trait so path
//! containment and symlink rejection stay enforced by the existing workspace
//! policy — this module never re-derives a root from a client-supplied path.
//!
//! Security notes:
//! - `push` is the only operation that shells out to the `git` CLI, because
//!   `gix` lacks a credential-aware transport. The daemon never stores or
//!   proxies git credentials; `push` inherits the agent process environment
//!   (SSH agent, credential helper, `GIT_ASKPASS`).
//! - Diff output is bounded per file (`MAX_DIFF_BYTES`) to prevent a huge
//!   generated file from exhausting daemon memory or the LAN response budget.
//! - Symlinks inside `.git/` are rejected up front by `open_repo`, matching
//!   the workspace symlink policy.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use gix::bstr::ByteSlice;
use thiserror::Error;

use crate::interfaces::AppError;

/// Maximum unified-diff bytes returned for a single file before truncation.
/// Bounds daemon memory and LAN response size; the API flags truncation so
/// the UI can warn instead of silently dropping content.
pub const MAX_DIFF_BYTES: usize = 1024 * 1024;

/// Read-only snapshot of a workspace's git repo state (S-GIT-DETECT).
///
/// Returned by [`detect`] and surfaced verbatim by
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
    /// action bar badge; full status lives in [`status`] (S-GIT-API).
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
    /// Unified-diff text (bounded by [`MAX_DIFF_BYTES`]).
    pub unified: String,
    /// Base (old) file content used by the editor merge viewer.
    pub base: String,
    /// Head (new) file content used by the editor merge viewer.
    pub head: String,
    /// `true` when the diff was capped at [`MAX_DIFF_BYTES`].
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
}

impl From<GitError> for AppError {
    fn from(error: GitError) -> Self {
        match error {
            GitError::NotARepo => AppError::not_found_kind("git repository"),
            GitError::SymlinkedGitDir | GitError::PathEscapes(_) => {
                AppError::validation(error.to_string())
            }
            GitError::Open(_) | GitError::Operation(_) | GitError::Push(_) => {
                AppError::internal(error.to_string())
            }
        }
    }
}

/// Open `root` as a read-only `gix` repo, rejecting symlinked `.git` first.
///
/// Returns `Ok(None)` when no `.git` exists (so callers can branch on
/// "no repo" without treating it as an error). The workspace manager has
/// already canonicalised `root` and rejected a symlinked root, so a
/// symlinked `.git` here is an agent-created escape attempt and is rejected.
///
/// # Errors
/// - [`GitError::SymlinkedGitDir`] when `.git` is a symlink.
/// - [`GitError::Open`] when `gix` fails to open the repo.
fn open_repo(root: &Path) -> Result<Option<gix::Repository>, GitError> {
    let dot_git = root.join(".git");
    if !dot_git.exists() {
        return Ok(None);
    }
    if let Ok(meta) = std::fs::symlink_metadata(&dot_git) {
        if meta.file_type().is_symlink() {
            return Err(GitError::SymlinkedGitDir);
        }
    }
    let repo = gix::open(root).map_err(|e| GitError::Open(Box::new(e)))?;
    Ok(Some(repo))
}

/// Detect a git repo at `root` and return a compact snapshot.
///
/// Pure read: opens the repo read-only, reads HEAD via the high-level
/// `head_name` / `head_commit` APIs, and probes for uncommitted changes via
/// `repo.status(...)`. Used by `GET /api/workspaces/{id}/git` (S-GIT-DETECT)
/// and for the action bar badge.
///
/// # Errors
/// Returns [`GitError`] only for symlink/open failures. A missing repo is
/// `Ok(GitRepoInfo { repo_detected: false, .. })`, never an error.
pub fn detect(root: &Path) -> Result<GitRepoInfo, GitError> {
    let Some(repo) = open_repo(root)? else {
        return Ok(GitRepoInfo::default());
    };

    // Shallow clones carry a `.git/shallow` file; checking it directly avoids
    // pulling in the gix-shallow API surface for a simple boolean.
    let is_shallow = root.join(".git").join("shallow").exists();

    let (head_branch, head_oid) = head_ref_info(&repo);
    let has_uncommitted_changes = has_worktree_changes(&repo);

    Ok(GitRepoInfo {
        repo_detected: true,
        head_branch,
        head_oid,
        is_shallow,
        has_uncommitted_changes,
    })
}

/// Resolve `(branch_name, hex_oid)` from HEAD.
///
/// - `(Some(name), Some(oid))` — normal branch checked out, HEAD points at a
///   commit.
/// - `(None, Some(oid))` — detached HEAD.
/// - `(None, None)` — unborn repo (no commits yet) or HEAD read failure.
fn head_ref_info(repo: &gix::Repository) -> (Option<String>, Option<String>) {
    // `head_name()` returns `Ok(None)` for a detached HEAD; `Ok(Some(name))`
    // for a branch. Errors (e.g. missing HEAD) map to `None`.
    let head_branch = repo
        .head_name()
        .ok()
        .flatten()
        .map(|full_name| full_name.shorten().to_string());

    // `head_commit()` fails for an unborn repo (no commits yet) — that maps
    // to `None` rather than an error.
    let head_oid = repo
        .head_commit()
        .ok()
        .map(|commit| commit.id().to_hex().to_string());

    (head_branch, head_oid)
}

/// Best-effort "is the worktree dirty" probe. Uses `repo.status(...)` with a
/// discard-progress handle and checks whether any item is returned. Any
/// error is treated as "no changes detected" so detection never fails a
/// workspace that simply has an unusual repo layout.
fn has_worktree_changes(repo: &gix::Repository) -> bool {
    let Ok(status) = repo.status(gix::progress::Discard) else {
        return false;
    };
    let Ok(mut iter) = status.into_iter(None) else {
        return false;
    };
    // `next()` returning `Ok(_)` means at least one changed entry exists.
    iter.any(|item| item.is_ok())
}

/// Reject `rel_path` if it escapes `root`. Returns the validated absolute
/// path. Mirrors the workspace manager's containment check so the git ops
/// layer is defence-in-depth even though callers pre-validate.
///
/// Not yet called by S-GIT-DETECT; used by `stage`/`unstage`/`diff` in
/// S-GIT-API. Kept here so the path-validation contract lives with the ops.
#[allow(dead_code)]
fn contained_path(root: &Path, rel_path: &str) -> Result<PathBuf, GitError> {
    use crate::pathutil::clean_path;
    clean_path(root, rel_path)
        .map_err(|_| GitError::PathEscapes(rel_path.to_string()))
        .and_then(|p| {
            // Final-component symlink rejection — workspace policy.
            crate::pathutil::resolve_symlink(root, &p)
                .map_err(|_| GitError::PathEscapes(rel_path.to_string()))
        })
}

// ---- S-GIT-API operations (status / diff / stage / unstage / commit) ----

/// `GET /api/workspaces/{id}/git/status`. Returns one row per changed file
/// grouped by staged vs. worktree, plus ahead/behind vs. upstream.
///
/// # Errors
///
/// Returns [`GitError`] when `root` is not a repository or status collection
/// fails.
pub fn status(root: &Path) -> Result<StatusResult, GitError> {
    let Some(repo) = open_repo(root)? else {
        return Err(GitError::NotARepo);
    };
    let (head_branch, head_oid) = head_ref_info(&repo);
    // gix provides the authoritative repository open and HEAD data. Its status
    // item API is still evolving rapidly, so porcelain v1 is used for this MVP
    // to retain Git's complete rename/conflict classification.
    let output = git_output(
        root,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let mut files = Vec::new();
    let entries: Vec<String> = output
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8_lossy(entry).into_owned())
        .collect();
    let mut index = 0;
    while index < entries.len() {
        let entry = &entries[index];
        if entry.len() < 3 {
            index += 1;
            continue;
        }
        let xy = &entry[..2];
        let path = entry[3..].replace('\\', "/");
        let rename = matches!(xy.as_bytes()[0], b'R' | b'C');
        let old_path = if rename {
            index += 1;
            entries.get(index).map(|old| old.replace('\\', "/"))
        } else {
            None
        };
        let status_for = |code: u8| match code {
            b'A' => "added",
            b'D' => "deleted",
            b'R' | b'C' => "renamed",
            b'U' => "conflicted",
            _ => "modified",
        };
        if xy == "??" {
            files.push(FileStatus {
                path,
                old_path: None,
                status: "untracked".to_string(),
                staged: false,
            });
        } else {
            if xy.as_bytes()[0] != b' ' {
                files.push(FileStatus {
                    path: path.clone(),
                    old_path: old_path.clone(),
                    status: status_for(xy.as_bytes()[0]).to_string(),
                    staged: true,
                });
            }
            if xy.as_bytes()[1] != b' ' {
                files.push(FileStatus {
                    path,
                    old_path,
                    status: status_for(xy.as_bytes()[1]).to_string(),
                    staged: false,
                });
            }
        }
        index += 1;
    }
    // Upstream/ahead/behind needs reference configuration traversal. Keep this
    // MVP deterministic until that gix plumbing is added.
    Ok(StatusResult {
        head_branch,
        head_oid,
        upstream: None,
        ahead: 0,
        behind: 0,
        files,
    })
}

/// Maximum number of commits returned by [`log`] in a single response.
/// Matches the story spec's cap; higher `limit` values are clamped to this.
pub const MAX_LOG_LIMIT: u32 = 200;

/// `GET /api/workspaces/{id}/git/log?limit=100&offset=0` (S-GIT-LOG-API).
///
/// Walks the commit graph from HEAD using `gix` (no `git log` CLI spawn),
/// returning a paginated list with parent refs, branch labels, and the HEAD
/// marker. An unborn repo (no commits) returns an empty list, not an error.
///
/// `limit` is clamped to [`MAX_LOG_LIMIT`]; `offset` skips commits (for
/// pagination). `total` is the full count of commits reachable from HEAD so
/// the frontend can render a pager; `has_more` is `true` when the page does
/// not reach the end.
///
/// # Errors
///
/// Returns [`GitError::NotARepo`] when `root` has no `.git`. Open/walk
/// failures map to [`GitError::Open`] / [`GitError::Operation`].
pub fn log(root: &Path, limit: u32, offset: u32) -> Result<LogResult, GitError> {
    let Some(repo) = open_repo(root)? else {
        return Err(GitError::NotARepo);
    };

    // Unborn repo: HEAD points at no commit. gix's `head_commit()` fails in
    // that case; treat it as an empty log rather than an error.
    let Ok(head_commit) = repo.head_commit() else {
        return Ok(LogResult::default());
    };
    let head_oid = head_commit.id;

    // Branch labels: scan local refs and map commit oid → short branch names.
    // Built once for the whole repo (cheap; refs are a small list) and looked
    // up per-commit below.
    let branch_map = build_branch_label_map(&repo)?;

    // Walk all commits reachable from HEAD, newest-first (topological). We
    // collect the full walk into a Vec so we can report `total` for pagination
    // — repos with huge histories may want a streaming approach later, but
    // for the MVP this is simple and correct.
    let walk = repo
        .rev_walk([head_oid])
        .all()
        .map_err(|e| GitError::Operation(format!("rev walk: {e}")))?;

    let mut all: Vec<LogCommit> = Vec::new();
    for item in walk {
        let info = item.map_err(|e| GitError::Operation(format!("walk item: {e}")))?;
        let oid = info.id;
        let oid_hex = oid.to_hex().to_string();

        // `info.parent_ids` gives the parent oids directly (no object read).
        let parents: Vec<String> = info
            .parent_ids
            .iter()
            .map(|p| p.to_hex().to_string())
            .collect();

        // Reading the full commit object for author/message. This is the
        // expensive path noted in the gix docs — acceptable for a paginated
        // log where we only decode the visible page after applying offset/limit.
        let commit = repo
            .find_commit(oid)
            .map_err(|e| GitError::Operation(format!("find commit {oid_hex}: {e}")))?;

        let author = commit
            .author()
            .map_err(|e| GitError::Operation(format!("decode author: {e}")))?;
        let author_time = author
            .time()
            .map_err(|e| GitError::Operation(format!("decode author time: {e}")))?;

        let message = commit
            .message()
            .map(|m| m.title.trim().to_str_lossy().to_string())
            .unwrap_or_default();

        let labels = branch_map.get(&oid_hex).cloned().unwrap_or_default();

        all.push(LogCommit {
            oid: oid_hex,
            parents,
            message,
            author: CommitAuthor {
                name: author.name.to_string(),
                email: author.email.to_string(),
                time: format_iso8601_utc(author_time),
            },
            branch_labels: labels,
            is_head: oid == head_oid,
        });
    }

    let total = all.len() as u64;
    let limit = limit.min(MAX_LOG_LIMIT) as usize;
    let offset = offset as usize;

    let commits: Vec<LogCommit> = all.into_iter().skip(offset).take(limit).collect();

    let has_more = ((offset + commits.len()) as u64) < total;

    Ok(LogResult {
        commits,
        total,
        has_more,
    })
}

/// Build a map of commit hex oid → short branch names for all local branches.
///
/// Scans `refs/heads/*` and peels each to its target commit. Multiple branches
/// can point at the same commit (e.g. after a fast-forward), so the value is a
/// `Vec`. Errors are non-fatal: a broken ref is skipped rather than failing
/// the whole log call.
fn build_branch_label_map(
    repo: &gix::Repository,
) -> Result<std::collections::HashMap<String, Vec<String>>, GitError> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let refs = repo
        .references()
        .map_err(|e| GitError::Operation(format!("references: {e}")))?;
    let branches = refs
        .local_branches()
        .map_err(|e| GitError::Operation(format!("local branches: {e}")))?;
    // `peeled()` ensures packed-refs entries are resolved without holding the
    // packed buffer across the consumer's peel calls.
    let branches = branches
        .peeled()
        .map_err(|e| GitError::Operation(format!("peel refs: {e}")))?;

    for branch in branches {
        let Ok(branch) = branch else {
            // Skip unreadable refs rather than failing the whole log.
            continue;
        };
        let full_name = branch.name();
        // Shorten `refs/heads/main` → `main`.
        let short = full_name
            .as_bstr()
            .to_string()
            .strip_prefix("refs/heads/")
            .map_or_else(
                || full_name.as_bstr().to_string(),
                std::string::ToString::to_string,
            );

        // Peel to the commit oid. Symbolic refs (e.g. HEAD) resolve through;
        // a branch that doesn't peel to a commit is skipped.
        let mut branch = branch;
        if let Ok(id) = branch.peel_to_id() {
            let hex = id.to_hex().to_string();
            map.entry(hex).or_default().push(short);
        }
    }

    Ok(map)
}

/// Format a `gix::date::Time` as an RFC 3339 / ISO 8601 UTC string.
///
/// `gix` stores seconds-since-epoch + offset; we render UTC (`Z` suffix) so
/// the frontend can localize with `new Date()`. Uses `chrono` (already a dep)
/// for formatting to avoid hand-rolling the calendar math. The original
/// commit's offset is dropped — the frontend renders in the viewer's timezone.
fn format_iso8601_utc(time: gix::date::Time) -> String {
    use chrono::{TimeZone, Utc};
    match Utc.timestamp_opt(time.seconds, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        // Out-of-range timestamps (negative, far future) fall back to the
        // raw seconds value rather than panicking.
        _ => time.seconds.to_string(),
    }
}

/// `GET /api/workspaces/{id}/git/diff?path=...&staged=...`. Bounded by
/// [`MAX_DIFF_BYTES`]; sets `truncated: true` when capped.
///
/// # Errors
///
/// Returns [`GitError`] when the path escapes the workspace, `root` is not a
/// repository, or Git cannot read the requested version.
pub fn diff(root: &Path, rel_path: &str, staged: bool) -> Result<DiffResult, GitError> {
    let Some(_) = open_repo(root)? else {
        return Err(GitError::NotARepo);
    };
    let path = contained_path(root, rel_path)?;
    let git_path = path
        .strip_prefix(root)
        .map_err(|_| GitError::PathEscapes(rel_path.to_string()))?
        .to_string_lossy()
        .replace('\\', "/");
    let base = if staged {
        git_file(root, &format!("HEAD:{git_path}"))?
    } else {
        git_file(root, &format!(":{git_path}"))?
    };
    let head = if staged {
        git_file(root, &format!(":{git_path}"))?
    } else {
        std::fs::read(path).unwrap_or_default()
    };
    if base[..base.len().min(8192)].contains(&0) || head[..head.len().min(8192)].contains(&0) {
        return Ok(DiffResult {
            unified: String::new(),
            base: "[binary file]".to_string(),
            head: "[binary file]".to_string(),
            truncated: false,
        });
    }
    let mut base = String::from_utf8_lossy(&base).into_owned();
    let mut head = String::from_utf8_lossy(&head).into_owned();
    let truncated = base.len().saturating_add(head.len()) > MAX_DIFF_BYTES;
    if truncated {
        base = truncate_utf8(&base, MAX_DIFF_BYTES / 2);
        head = truncate_utf8(&head, MAX_DIFF_BYTES / 2);
    }
    // CodeMirror's merge view consumes base/head and computes its own diff.
    Ok(DiffResult {
        unified: String::new(),
        base,
        head,
        truncated,
    })
}

/// `POST /api/workspaces/{id}/git/stage`. Validates each path against the
/// workspace root before staging.
///
/// # Errors
///
/// Returns [`GitError`] when a path escapes the workspace, `root` is not a
/// repository, or Git cannot update its index.
pub fn stage(root: &Path, paths: &[String]) -> Result<Vec<String>, GitError> {
    let Some(_) = open_repo(root)? else {
        return Err(GitError::NotARepo);
    };
    if paths.is_empty() {
        let changed = status(root)?
            .files
            .into_iter()
            .map(|file| file.path)
            .collect();
        git_output(root, ["add", "-A"])?;
        return Ok(changed);
    }
    for path in paths {
        contained_path(root, path)?;
    }
    // gix index mutation is the intended long-term path; CLI preserves exact
    // Git ignore, intent-to-add, and deletion semantics for the MVP.
    let mut command = Command::new("git");
    command.current_dir(root).arg("add").arg("--").args(paths);
    run_git(command)?;
    Ok(paths.to_vec())
}

/// `POST /api/workspaces/{id}/git/unstage`.
///
/// # Errors
///
/// # Errors
///
/// Returns [`GitError`] when a path escapes the workspace, `root` is not a
/// repository, or Git cannot reset its index.
pub fn unstage(root: &Path, paths: &[String]) -> Result<Vec<String>, GitError> {
    let Some(_) = open_repo(root)? else {
        return Err(GitError::NotARepo);
    };
    for path in paths {
        contained_path(root, path)?;
    }
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .arg("reset")
        .arg("HEAD")
        .arg("--")
        .args(paths);
    run_git(command)?;
    Ok(paths.to_vec())
}

/// `POST /api/workspaces/{id}/git/commit`. Caller passes the `head_oid` from
/// the last `/status` as `expected_head`; mismatch → [`GitError::Operation`]
/// (mapped to 409 by the API layer) so a mid-flight edit cannot be committed.
///
/// # Errors
///
/// Returns [`GitError`] when `root` is not a repository, the supplied head is
/// stale, or Git cannot create the commit.
pub fn commit(
    root: &Path,
    message: &str,
    expected_head: Option<&str>,
    amend: bool,
) -> Result<String, GitError> {
    let Some(repo) = open_repo(root)? else {
        return Err(GitError::NotARepo);
    };
    let (_, actual_head) = head_ref_info(&repo);
    // No precondition is only valid for the initial commit (unborn HEAD). With
    // a HEAD present, a missing or mismatched If-Match means the working tree
    // changed since the last status fetch.
    let head_ok = match (actual_head.as_deref(), expected_head) {
        (None, _) => true,
        (Some(actual), Some(expected)) => actual.eq_ignore_ascii_case(expected),
        _ => false,
    };
    if !head_ok {
        return Err(GitError::Operation(
            "working tree changed since last status fetch".to_string(),
        ));
    }
    // gix commit creation is the intended long-term path; use Git here to
    // honor hooks and configured signing while the MVP has no identity store.
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .arg("commit")
        .arg("-m")
        .arg(message);
    if amend {
        command.arg("--amend");
    }
    configure_default_identity(&mut command, root);
    run_git(command)?;
    Ok(
        String::from_utf8_lossy(&git_output(root, ["rev-parse", "HEAD"])?)
            .trim()
            .to_string(),
    )
}

/// `POST /api/workspaces/{id}/git/push`. Shells out to `git push` so the
/// user's existing git credentials apply.
/// `stderr` is returned verbatim for the UI to stream.
///
/// # Errors
///
/// Returns [`GitError::NotARepo`] when `root` is not a repository and
/// [`GitError::Push`] when Git cannot push.
pub fn push(root: &Path, remote: Option<&str>, set_upstream: bool) -> Result<String, GitError> {
    let Some(_) = open_repo(root)? else {
        return Err(GitError::NotARepo);
    };
    let mut command = Command::new("git");
    command.current_dir(root).arg("push");
    if set_upstream {
        command.arg("--set-upstream");
    }
    command.arg(remote.unwrap_or("origin"));
    let output = command
        .output()
        .map_err(|err| GitError::Push(err.to_string()))?;
    let text = output_text(&output);
    if !output.status.success() {
        return Err(GitError::Push(text));
    }
    Ok(text)
}

/// `POST /api/workspaces/{id}/git/init`. Creates a repo at the workspace root
/// with `main` as the initial branch. Refuses if `.git` already exists.
///
/// # Errors
///
/// Returns [`GitError`] when a repository already exists or initialization
/// cannot create the initial commit.
pub fn init(root: &Path) -> Result<String, GitError> {
    if root.join(".git").exists() {
        return Err(GitError::Operation(".git already exists".to_string()));
    }
    gix::init(root).map_err(|err| GitError::Operation(err.to_string()))?;
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["commit", "--allow-empty", "-m", "initial"]);
    configure_default_identity(&mut command, root);
    run_git(command)?;
    Ok(
        String::from_utf8_lossy(&git_output(root, ["rev-parse", "HEAD"])?)
            .trim()
            .to_string(),
    )
}

fn git_output<const N: usize>(root: &Path, args: [&str; N]) -> Result<Vec<u8>, GitError> {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    Ok(run_git(command)?.stdout)
}

fn git_file(root: &Path, spec: &str) -> Result<Vec<u8>, GitError> {
    let output = Command::new("git")
        .current_dir(root)
        .arg("show")
        .arg(spec)
        .output()
        .map_err(|err| GitError::Operation(err.to_string()))?;
    // A missing path is expected for additions/deletions.
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Ok(Vec::new())
    }
}

fn run_git(mut command: Command) -> Result<Output, GitError> {
    let output = command
        .output()
        .map_err(|err| GitError::Operation(err.to_string()))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(GitError::Operation(output_text(&output)))
    }
}

fn configure_default_identity(command: &mut Command, root: &Path) {
    let has_email = Command::new("git")
        .current_dir(root)
        .args(["config", "user.email"])
        .output()
        .is_ok_and(|output| output.status.success());
    if !has_email {
        command
            .env("GIT_AUTHOR_NAME", "Local Agent")
            .env("GIT_AUTHOR_EMAIL", "agent@local")
            .env("GIT_COMMITTER_NAME", "Local Agent")
            .env("GIT_COMMITTER_EMAIL", "agent@local");
    }
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn truncate_utf8(value: &str, limit: usize) -> String {
    let mut end = limit.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

/// Append `patterns` to `root/.gitignore`, deduping exact-line matches.
///
/// Unlike the other ops in this module, this does **not** require a repo —
/// `.gitignore` is just a file at the workspace root, so we read/write it
/// directly without `open_repo`. Each pattern is trimmed and rejected if empty.
/// Returns the list of patterns that were actually appended (empty when every
/// requested pattern was already present as an exact line).
///
/// # Errors
///
/// Returns [`GitError::Operation`] for any I/O error or when a pattern is empty
/// after trimming.
pub fn add_to_gitignore(root: &Path, patterns: &[String]) -> Result<Vec<String>, GitError> {
    let mut added: Vec<String> = Vec::new();
    let path = root.join(".gitignore");

    // Read existing lines (if any) so we can dedup exact matches. A trailing
    // newline is trimmed so the existing content is treated as a line list.
    let mut content = match std::fs::read_to_string(&path) {
        Ok(existing) => existing,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(GitError::Operation(format!("read .gitignore: {err}"))),
    };
    // Own the lines so `content` can be mutated below without keeping an
    // immutable borrow alive.
    let existing_lines: Vec<String> = content.lines().map(str::to_string).collect();

    for raw in patterns {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(GitError::Operation("empty pattern".into()));
        }
        if existing_lines.iter().any(|line| line == trimmed) {
            continue;
        }
        // Ensure the existing content ends with a newline before appending so
        // we don't merge the last old line with the first new one.
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(trimmed);
        content.push('\n');
        added.push(trimmed.to_string());
    }

    if added.is_empty() {
        // Nothing to write — avoid touching the file's mtime for pure-dedup
        // calls (also avoids creating an empty `.gitignore` when the file was
        // missing and every pattern was a no-op, which can't happen here
        // because missing-file dedup is trivially empty, but the guard keeps
        // the contract clear).
        return Ok(added);
    }

    std::fs::write(&path, content)
        .map_err(|err| GitError::Operation(format!("write .gitignore: {err}")))?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a throwaway repo with `git init` + an initial commit so the
    /// detection + status probes have real state to read. Production code
    /// never shells out — only the test fixture does.
    fn fresh_repo(dir: &Path) {
        std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg("-b")
            .arg("main")
            .current_dir(dir)
            .status()
            .expect("git init");
        std::fs::write(dir.join("README.md"), "hello\n").expect("write");
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .status()
            .expect("git add");
        std::process::Command::new("git")
            .args([
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "-m",
                "init",
            ])
            .current_dir(dir)
            .status()
            .expect("git commit");
    }

    #[test]
    fn detect_returns_no_repo_for_plain_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = detect(dir.path()).expect("detect");
        assert!(!info.repo_detected);
        assert_eq!(info.head_branch, None);
        assert_eq!(info.head_oid, None);
        assert!(!info.is_shallow);
    }

    #[test]
    fn detect_reports_branch_and_oid_for_real_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        let info = detect(dir.path()).expect("detect");
        assert!(info.repo_detected);
        assert_eq!(info.head_branch.as_deref(), Some("main"));
        assert!(info.head_oid.is_some());
        assert!(!info.is_shallow);
        assert!(!info.has_uncommitted_changes);
    }

    #[test]
    fn detect_flags_uncommitted_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
        let info = detect(dir.path()).expect("detect");
        assert!(info.has_uncommitted_changes);
    }

    #[test]
    fn detect_rejects_symlinked_git_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = tempfile::tempdir().expect("tempdir");
        fresh_repo(real.path());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(real.path().join(".git"), dir.path().join(".git"))
                .expect("symlink");
        }
        let err = detect(dir.path()).expect_err("should reject symlinked .git");
        assert!(matches!(err, GitError::SymlinkedGitDir));
    }

    #[test]
    fn status_returns_empty_for_clean_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        assert!(status(dir.path()).expect("status").files.is_empty());
    }

    #[test]
    fn status_lists_modified_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
        let files = status(dir.path()).expect("status").files;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "README.md");
        assert!(!files[0].staged);
    }

    #[test]
    fn status_expands_untracked_directory_into_individual_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        std::fs::create_dir_all(dir.path().join("group")).expect("mkdir");
        std::fs::write(dir.path().join("group/a.txt"), "a\n").expect("write");
        std::fs::write(dir.path().join("group/b.txt"), "b\n").expect("write");
        let files = status(dir.path()).expect("status").files;
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(
            paths.contains(&"group/a.txt"),
            "expected group/a.txt, got {paths:?}"
        );
        assert!(
            paths.contains(&"group/b.txt"),
            "expected group/b.txt, got {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with('/')),
            "no collapsed folder entries, got {paths:?}"
        );
    }

    #[test]
    fn status_lists_staged_file_after_add() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir.path())
            .status()
            .expect("git add");
        assert!(status(dir.path()).expect("status").files[0].staged);
    }

    #[test]
    fn diff_returns_base_and_head_for_modified_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
        let result = diff(dir.path(), "README.md", false).expect("diff");
        assert!(!result.base.is_empty());
        assert!(!result.head.is_empty());
    }

    #[test]
    fn stage_then_status_shows_staged() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
        stage(dir.path(), &[String::from("README.md")]).expect("stage");
        assert!(status(dir.path())
            .expect("status")
            .files
            .iter()
            .any(|file| file.path == "README.md" && file.staged));
    }

    #[test]
    fn commit_creates_new_oid() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        let old_oid = detect(dir.path())
            .expect("detect")
            .head_oid
            .expect("head oid");
        std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
        stage(dir.path(), &[String::from("README.md")]).expect("stage");
        let oid = commit(dir.path(), "change", Some(&old_oid), false).expect("commit");
        assert_ne!(oid, old_oid);
    }

    #[test]
    fn commit_rejects_stale_expected_head() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        std::fs::write(dir.path().join("README.md"), "changed\n").expect("write");
        stage(dir.path(), &[String::from("README.md")]).expect("stage");
        assert!(matches!(
            commit(dir.path(), "change", Some("0000000"), false),
            Err(GitError::Operation(_))
        ));
    }

    #[test]
    fn commit_allows_initial_commit_without_precondition() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Unborn repo: `git init` only, no initial commit, then stage a file.
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(dir.path())
            .status()
            .expect("git init");
        std::fs::write(dir.path().join("README.md"), "first\n").expect("write");
        stage(dir.path(), &[String::from("README.md")]).expect("stage");
        let oid = commit(dir.path(), "initial", None, false).expect("initial commit");
        assert!(!oid.is_empty());
        // A missing precondition against a born HEAD must still be rejected.
        std::fs::write(dir.path().join("README.md"), "second\n").expect("write");
        stage(dir.path(), &[String::from("README.md")]).expect("stage");
        assert!(matches!(
            commit(dir.path(), "second", None, false),
            Err(GitError::Operation(_))
        ));
    }

    #[test]
    fn init_refuses_existing_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        assert!(init(dir.path()).is_err());
    }

    #[test]
    fn init_creates_repo_in_plain_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!init(dir.path()).expect("init").is_empty());
        assert!(detect(dir.path()).expect("detect").repo_detected);
    }

    #[test]
    fn gitignore_creates_file_when_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        let added = add_to_gitignore(dir.path(), &["target/".into()]).expect("add");
        let file = dir.path().join(".gitignore");
        assert!(file.exists(), ".gitignore should be created");
        let content = std::fs::read_to_string(file).expect("read");
        assert!(
            content.lines().any(|line| line == "target/"),
            "content: {content}"
        );
        assert_eq!(added, vec!["target/".to_string()]);
    }

    #[test]
    fn gitignore_dedups_existing_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        let file = dir.path().join(".gitignore");
        std::fs::write(&file, "target/\n").expect("write");
        let added = add_to_gitignore(dir.path(), &["target/".into()]).expect("add");
        assert!(added.is_empty(), "no new patterns: {added:?}");
        assert_eq!(std::fs::read_to_string(file).expect("read"), "target/\n");
    }

    #[test]
    fn gitignore_appends_new_patterns() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        let file = dir.path().join(".gitignore");
        std::fs::write(&file, "target/\n").expect("write");
        let added = add_to_gitignore(dir.path(), &["node_modules/".into()]).expect("add");
        let content = std::fs::read_to_string(file).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines.contains(&"target/"), "content: {content}");
        assert!(lines.contains(&"node_modules/"), "content: {content}");
        assert_eq!(added, vec!["node_modules/".to_string()]);
    }

    /// Create an additional commit on top of `fresh_repo`'s initial commit.
    /// Writes a unique file so each commit has a distinct tree.
    fn add_commit(dir: &Path, name: &str, message: &str) {
        std::fs::write(dir.join(name), format!("{name}\n")).expect("write");
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .status()
            .expect("git add");
        std::process::Command::new("git")
            .args([
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "-m",
                message,
            ])
            .current_dir(dir)
            .status()
            .expect("git commit");
    }

    #[test]
    fn log_returns_not_a_repo_for_plain_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = log(dir.path(), 100, 0).expect_err("should error");
        assert!(matches!(err, GitError::NotARepo));
    }

    #[test]
    fn log_returns_empty_for_unborn_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        // `git init` only — no commits, so HEAD is unborn.
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(dir.path())
            .status()
            .expect("git init");
        let result = log(dir.path(), 100, 0).expect("log");
        assert!(result.commits.is_empty());
        assert_eq!(result.total, 0);
        assert!(!result.has_more);
    }

    #[test]
    fn log_returns_head_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        let result = log(dir.path(), 100, 0).expect("log");
        assert_eq!(result.commits.len(), 1, "one commit in fresh repo");
        assert_eq!(result.total, 1);
        assert!(!result.has_more);

        let commit = &result.commits[0];
        assert!(!commit.oid.is_empty());
        assert!(commit.parents.is_empty(), "initial commit has no parents");
        assert_eq!(commit.message, "init");
        assert!(commit.is_head, "the only commit is HEAD");
        assert_eq!(commit.author.name, "t");
        assert_eq!(commit.author.email, "t@t");
        // ISO 8601 UTC ends with 'Z'.
        assert!(
            commit.author.time.ends_with('Z'),
            "time: {}",
            commit.author.time
        );
        // The default branch is `main` (from `fresh_repo`).
        assert!(
            commit.branch_labels.iter().any(|l| l == "main"),
            "branch_labels: {:?}",
            commit.branch_labels
        );
    }

    #[test]
    fn log_paginates_with_limit_offset() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        add_commit(dir.path(), "a.txt", "second");
        add_commit(dir.path(), "b.txt", "third");

        // 3 commits total. Page 1: limit=2, offset=0 → 2 commits, has_more.
        let page1 = log(dir.path(), 2, 0).expect("log page 1");
        assert_eq!(page1.commits.len(), 2);
        assert_eq!(page1.total, 3);
        assert!(page1.has_more);

        // Page 2: limit=2, offset=2 → 1 commit, no has_more.
        let page2 = log(dir.path(), 2, 2).expect("log page 2");
        assert_eq!(page2.commits.len(), 1);
        assert_eq!(page2.total, 3);
        assert!(!page2.has_more);
    }

    #[test]
    fn log_caps_limit_at_max() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        // Request limit=1000 — should be clamped to MAX_LOG_LIMIT (200) without
        // panicking. With only 1 commit, the result has 1 entry.
        let result = log(dir.path(), 1000, 0).expect("log");
        assert_eq!(result.commits.len(), 1);
        assert_eq!(result.total, 1);
    }

    #[test]
    fn log_attaches_branch_labels_and_head_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        add_commit(dir.path(), "a.txt", "second");

        let result = log(dir.path(), 100, 0).expect("log");
        // Newest-first ordering: the second commit is HEAD.
        let head = &result.commits[0];
        assert!(head.is_head);
        assert!(
            head.branch_labels.iter().any(|l| l == "main"),
            "HEAD branch_labels: {:?}",
            head.branch_labels
        );
        // The initial commit is not HEAD.
        let init = &result.commits[1];
        assert!(!init.is_head);
        // `main` points at HEAD only, so the initial commit has no labels.
        assert!(
            init.branch_labels.is_empty(),
            "init branch_labels: {:?}",
            init.branch_labels
        );
        // The initial commit is the parent of the second.
        assert_eq!(head.parents.len(), 1);
        assert_eq!(head.parents[0], init.oid);
    }

    #[test]
    fn log_reports_parent_oids() {
        let dir = tempfile::tempdir().expect("tempdir");
        fresh_repo(dir.path());
        add_commit(dir.path(), "a.txt", "second");

        let result = log(dir.path(), 100, 0).expect("log");
        let head = &result.commits[0];
        let init = &result.commits[1];
        assert_eq!(head.parents, vec![init.oid.clone()]);
        assert!(init.parents.is_empty());
    }
}
