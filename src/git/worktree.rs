use std::path::Path;
use std::process::Command;

use super::cli::{configure_default_identity, git_file, git_output, output_text, run_git};
use super::repo::{contained_path, head_ref_info, open_repo, status};
use super::{DiffResult, GitError};

/// Maximum unified-diff bytes returned for a single file before truncation.
/// Bounds daemon memory and LAN response size; the API flags truncation so
/// the UI can warn instead of silently dropping content.
pub const MAX_DIFF_BYTES: usize = 1024 * 1024;

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

/// `POST /api/workspaces/{id}/git/discard`. Restores tracked files to their
/// index/HEAD state via `git checkout -- <paths>` and deletes untracked files
/// via `std::fs::remove_file` (cross-platform, no `rm` spawn). Path containment
/// is pre-validated for every path before any mutation, so a batch with one
/// escaping path fails atomically without side effects.
///
/// Returns the count of paths processed (tracked + untracked).
///
/// # Errors
///
/// Returns [`GitError::NotARepo`] when `root` is not a repository,
/// [`GitError::PathEscapes`] when any path escapes the workspace, and
/// [`GitError::Operation`] for git CLI or filesystem failures.
pub fn discard(root: &Path, paths: &[String]) -> Result<usize, GitError> {
    let Some(_) = open_repo(root)? else {
        return Err(GitError::NotARepo);
    };
    // Pre-validate every path before any mutation so a bad path in a batch
    // fails atomically. `contained_path` supports non-existent final components
    // (deleted tracked files), validating the parent chain instead.
    for path in paths {
        contained_path(root, path)?;
    }

    // Classify each path as tracked or untracked. `git ls-files --error-unmatch`
    // exits non-zero for untracked paths, which is the expected classification
    // signal — not an error in this context.
    let mut tracked: Vec<&String> = Vec::new();
    let mut untracked: Vec<&String> = Vec::new();
    for path in paths {
        if is_tracked(root, path)? {
            tracked.push(path);
        } else {
            untracked.push(path);
        }
    }

    // Restore tracked files from the index in a single checkout command.
    if !tracked.is_empty() {
        let mut command = Command::new("git");
        command
            .current_dir(root)
            .arg("checkout")
            .arg("--")
            .args(&tracked);
        run_git(command)?;
    }

    // Delete untracked files via the filesystem (cross-platform, no shell `rm`).
    for path in &untracked {
        let full = contained_path(root, path)?;
        if full.exists() {
            if full.is_dir() {
                std::fs::remove_dir_all(&full)
                    .map_err(|err| GitError::Operation(format!("remove {path}: {err}")))?;
            } else {
                std::fs::remove_file(&full)
                    .map_err(|err| GitError::Operation(format!("remove {path}: {err}")))?;
            }
        }
    }

    Ok(tracked.len() + untracked.len())
}

/// Checks whether `path` is tracked by git (exists in the index).
/// `git ls-files --error-unmatch -- <path>` exits non-zero for untracked paths.
fn is_tracked(root: &Path, path: &str) -> Result<bool, GitError> {
    let output = Command::new("git")
        .current_dir(root)
        .arg("ls-files")
        .arg("--error-unmatch")
        .arg("--")
        .arg(path)
        .output()
        .map_err(|err| GitError::Operation(format!("ls-files: {err}")))?;
    Ok(output.status.success())
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
