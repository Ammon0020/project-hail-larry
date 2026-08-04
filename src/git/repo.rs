use std::path::{Path, PathBuf};

use super::cli::git_output;
use super::{FileStatus, GitError, GitRepoInfo, StatusResult};

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
pub(super) fn open_repo(root: &Path) -> Result<Option<gix::Repository>, GitError> {
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
pub(super) fn head_ref_info(repo: &gix::Repository) -> (Option<String>, Option<String>) {
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
/// Used by `stage`/`unstage`/`diff` in S-GIT-API. Kept here so the
/// path-validation contract lives with the operations.
pub(super) fn contained_path(root: &Path, rel_path: &str) -> Result<PathBuf, GitError> {
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
    // Upstream/ahead/behind needs reference configuration traversal. gix's
    // ref-config API is still awkward to use here; shell out to the CLI for
    // the small, well-defined `@{u}` + `rev-list --left-right --count` pair.
    // Errors are swallowed so status still works for repos without upstreams.
    let (upstream, ahead, behind) = match &head_branch {
        Some(_) => upstream_ahead_behind(root),
        None => (None, 0, 0),
    };
    // List local + remote-tracking branch short names. `--all` includes
    // `refs/remotes/*`; `%(refname:short)` strips to `main` / `origin/main`.
    // `HEAD` symbolic refs (e.g. `origin/HEAD`) are filtered out — they're
    // not checkoutable and just clutter the dropdown.
    let branches: Vec<String> = git_output(root, ["branch", "--all", "--format=%(refname:short)"])
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|name| !name.is_empty() && !name.ends_with("/HEAD") && name != "HEAD")
                .collect()
        })
        .unwrap_or_default();
    Ok(StatusResult {
        head_branch,
        head_oid,
        upstream,
        ahead,
        behind,
        branches,
        files,
    })
}

/// Resolve the upstream tracking branch and ahead/behind counts for `root`.
///
/// - `git rev-parse --abbrev-ref --symbolic-full-name @{u}` → upstream name
///   (fails when no upstream is configured).
/// - `git rev-list --left-right --count <upstream>...HEAD` → `behind\tahead`.
///
/// Any failure (no upstream, parse error, git missing) collapses to
/// `(None, 0, 0)` so [`status`] never breaks for a repo that simply lacks
/// tracking configuration.
fn upstream_ahead_behind(root: &Path) -> (Option<String>, u64, u64) {
    let Ok(upstream_bytes) = git_output(
        root,
        ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    ) else {
        return (None, 0, 0);
    };
    let upstream = String::from_utf8_lossy(&upstream_bytes).trim().to_string();
    if upstream.is_empty() {
        return (None, 0, 0);
    }
    let Ok(count_bytes) = git_output(
        root,
        [
            "rev-list",
            "--left-right",
            "--count",
            &format!("{upstream}...HEAD"),
        ],
    ) else {
        return (None, 0, 0);
    };
    let count = String::from_utf8_lossy(&count_bytes);
    // `--left-right --count` prints `<left>\t<right>` where left = commits in
    // upstream only (behind) and right = commits in HEAD only (ahead).
    let mut parts = count.split_whitespace();
    let behind = parts
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let ahead = parts
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    (Some(upstream), ahead, behind)
}
