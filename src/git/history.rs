use std::path::Path;
use std::process::Command;

use super::cli::output_text;
use super::repo::open_repo;
use super::{CommitAuthor, GitError, LogCommit, LogResult};

/// Maximum number of commits returned by [`log`] in a single response.
/// Matches the story spec's cap; higher `limit` values are clamped to this.
pub const MAX_LOG_LIMIT: u32 = 200;

/// `GET /api/workspaces/{id}/git/log?limit=100&offset=0` (S-GIT-LOG-API).
///
/// Returns a paginated slice of the commit history reachable from every local
/// branch head (plus the checked-out HEAD, in case it is detached). An unborn
/// repo (no commits) returns an empty list, not an error.
///
/// Walking the union of branch heads — not just `HEAD` — means a diverged
/// local branch's unique commits appear in the log even when it isn't
/// checked out, so the frontend can render the full branch topology. The
/// `is_head` flag is still set only on the commit `HEAD` actually points at.
///
/// Unlike a pure-`gix` walk that would have to decode every reachable commit
/// before applying `offset`/`limit`, this shells out to `git log` so the C
/// implementation (mmap + commitgraph) does the heavy lifting and native
/// `--skip`/`--max-count` pagination means only the visible page's author and
/// message metadata is ever decoded. On a 10k-commit repo a single page load
/// decodes ~100 commit objects instead of 10k.
///
/// `limit` is clamped to [`MAX_LOG_LIMIT`]; `offset` skips commits (for
/// pagination). `total` is the full count of commits reachable from the
/// union of branch heads so the frontend can render a pager; `has_more` is
/// `true` when the page does not reach the end.
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
    let head_hex = head_oid.to_hex().to_string();

    // Branch labels and tag labels are still built via gix refs — these are
    // small, cheap scans over `refs/heads/*` and `refs/tags/*` and don't touch
    // any commit objects. Only the visible page's commit metadata is decoded
    // (by `git log`, below); the label maps are looked up by oid.
    let branch_map = build_branch_refs(&repo)?;
    let tag_map = build_tag_refs(&repo)?;

    // Total count of commits reachable from local branches + HEAD, for the
    // pager. `git rev-list --count --branches` counts commits reachable from
    // local branch heads; if HEAD is detached (not on any branch) it is added
    // explicitly so its unique commits are counted too.
    let total = count_reachable(root, &head_hex, &branch_map)?;

    let limit = limit.min(MAX_LOG_LIMIT);

    // Fetch the visible page via `git log --date-order --skip=N --max-count=L`.
    // `--date-order` orders by commit date (newest first) while respecting
    // parent topology, so recent commits from all branches surface first.
    //
    // Format: %H|%P|%an|%ae|%at|%s  →  oid|parents|name|email|unix-seconds|subject.
    // We emit the author date as a unix timestamp (`%at`) and render UTC (`Z`
    // suffix) in Rust — matching the previous gix-based output and the
    // frontend's `new Date()` expectation. The subject is the final field
    // (splitn(6, '|')) so it may contain `|`. `--format=<value>` (not
    // `--format <value>`) is required so git doesn't treat the `%`-string as
    // a pathspec.
    let format = "--format=%H|%P|%an|%ae|%at|%s";
    let mut cmd = Command::new("git");
    cmd.current_dir(root)
        .arg("log")
        .arg("--date-order")
        .arg(format)
        .arg("--skip")
        .arg(offset.to_string())
        .arg("--max-count")
        .arg(limit.to_string());

    // `--branches` walks from all local branch heads. If HEAD is detached
    // (its oid isn't pointed to by any local branch), add `HEAD` as an extra
    // tip so its commits appear and `is_head` always has a match.
    cmd.arg("--branches");
    let head_on_branch = branch_map.contains_key(&head_hex);
    if !head_on_branch {
        cmd.arg("HEAD");
    }

    let output = cmd
        .output()
        .map_err(|e| GitError::Operation(format!("git log: {e}")))?;
    let text = output_text(&output);
    if !output.status.success() {
        return Err(GitError::Operation(text));
    }

    let commits: Vec<LogCommit> = text
        .lines()
        .filter_map(|line| parse_log_line(line, &branch_map, &tag_map, &head_hex))
        .collect();

    let has_more = (u64::from(offset) + commits.len() as u64) < total;

    Ok(LogResult {
        commits,
        total,
        has_more,
    })
}

/// Count commits reachable from local branches + HEAD via `git rev-list --count`.
///
/// `--branches` counts commits reachable from all local branch heads. When HEAD
/// is detached (its oid isn't any branch head) it is added as an extra tip so
/// its unique commits are included in the total — matching the walk the
/// `git log` page query performs.
fn count_reachable(
    root: &Path,
    head_hex: &str,
    branch_map: &BranchLabelMap,
) -> Result<u64, GitError> {
    let head_on_branch = branch_map.contains_key(head_hex);
    let mut cmd = Command::new("git");
    cmd.current_dir(root)
        .arg("rev-list")
        .arg("--count")
        .arg("--branches");
    if !head_on_branch {
        cmd.arg("HEAD");
    }
    let output = cmd
        .output()
        .map_err(|e| GitError::Operation(format!("rev-list count: {e}")))?;
    let text = output_text(&output);
    if !output.status.success() {
        return Err(GitError::Operation(text));
    }
    text.trim()
        .parse::<u64>()
        .map_err(|e| GitError::Operation(format!("parse count: {e}")))
}

/// Parse one `git log --format` line into a [`LogCommit`].
///
/// Expected format: `%H|%P|%an|%ae|%at|%s` →
/// `oid|parents|name|email|unix-seconds|subject`. The subject is the final
/// field and is allowed to contain `|` (we split into at most 6 parts). The
/// unix-seconds field is rendered to ISO 8601 UTC (`Z` suffix) via
/// [`format_iso8601_utc`]. Branch and tag labels are looked up by oid in the
/// prebuilt maps; `is_head` is set when the oid matches HEAD. Returns `None`
/// for malformed/truncated lines.
fn parse_log_line(
    line: &str,
    branch_map: &BranchLabelMap,
    tag_map: &BranchLabelMap,
    head_hex: &str,
) -> Option<LogCommit> {
    let parts: Vec<&str> = line.splitn(6, '|').collect();
    if parts.len() < 6 {
        return None;
    }
    let oid = parts[0].to_string();
    let parents = if parts[1].is_empty() {
        Vec::new()
    } else {
        parts[1].split(' ').map(String::from).collect()
    };
    let name = parts[2].to_string();
    let email = parts[3].to_string();
    // `%at` is unix seconds; render as UTC ISO 8601 to match the previous
    // gix-based output. A non-numeric timestamp falls back to the raw field
    // rather than dropping the commit.
    let time = parts[4]
        .parse::<i64>()
        .map_or_else(|_| parts[4].to_string(), format_iso8601_utc);
    let message = parts[5].trim().to_string();
    let labels = branch_map.get(&oid).cloned().unwrap_or_default();
    let tags = tag_map.get(&oid).cloned().unwrap_or_default();
    let is_head = oid == *head_hex;
    Some(LogCommit {
        oid,
        parents,
        message,
        author: CommitAuthor { name, email, time },
        branch_labels: labels,
        tag_labels: tags,
        is_head,
    })
}

/// Format a unix-seconds timestamp as an RFC 3339 / ISO 8601 UTC string (`Z`).
///
/// The frontend localizes with `new Date()`, which accepts the `Z` form. Uses
/// `chrono` (already a dep) for the calendar math. Out-of-range timestamps
/// (negative, far future) fall back to the raw seconds value rather than
/// panicking.
fn format_iso8601_utc(seconds: i64) -> String {
    use chrono::{TimeZone, Utc};
    match Utc.timestamp_opt(seconds, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        _ => seconds.to_string(),
    }
}

/// Build the branch label map: commit hex oid → short local branch names.
///
/// Scans `refs/heads/*` once and peels each to its target commit. Multiple
/// branches can point at the same commit (e.g. after a fast-forward), so the
/// value is a `Vec`. Errors are non-fatal: a broken ref is skipped rather than
/// failing the whole log call.
type BranchLabelMap = std::collections::HashMap<String, Vec<String>>;

fn build_branch_refs(repo: &gix::Repository) -> Result<BranchLabelMap, GitError> {
    let mut map: BranchLabelMap = std::collections::HashMap::new();
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
            let oid = id.detach();
            let hex = oid.to_hex().to_string();
            map.entry(hex).or_default().push(short);
        }
    }

    Ok(map)
}

/// Build the tag label map: commit hex oid → short tag names pointing at it.
///
/// Mirrors [`build_branch_refs`] but for `refs/tags/*`. Tags don't seed the
/// `rev_walk` tips — they only add labels to commits that already appear in the
/// walk — so this returns just the map (no tips). Annotated tags (which point
/// at a tag object that points at a commit) are peeled to their target commit
/// via `peel_to_id()`; multiple tags can point at the same commit, so the
/// value is a `Vec`. Broken refs are skipped rather than failing the log call.
fn build_tag_refs(repo: &gix::Repository) -> Result<BranchLabelMap, GitError> {
    let mut map: BranchLabelMap = std::collections::HashMap::new();
    let refs = repo
        .references()
        .map_err(|e| GitError::Operation(format!("references: {e}")))?;
    let tags = refs
        .tags()
        .map_err(|e| GitError::Operation(format!("tags: {e}")))?;
    // `peeled()` resolves packed-refs entries up front; without it the packed
    // buffer would be held across the consumer's peel calls and panic.
    let tags = tags
        .peeled()
        .map_err(|e| GitError::Operation(format!("peel tag refs: {e}")))?;

    for tag in tags {
        let Ok(mut tag) = tag else {
            // Skip unreadable refs rather than failing the whole log.
            continue;
        };
        let full_name = tag.name();
        // Shorten `refs/tags/v1.0.0` → `v1.0.0`.
        let short = full_name
            .as_bstr()
            .to_string()
            .strip_prefix("refs/tags/")
            .map_or_else(
                || full_name.as_bstr().to_string(),
                std::string::ToString::to_string,
            );

        // Peel to the target commit oid. Annotated tags point at a tag object
        // that points at a commit; `peel_to_id()` follows the chain. A tag that
        // doesn't peel to a commit is skipped.
        if let Ok(id) = tag.peel_to_id() {
            let oid = id.detach();
            let hex = oid.to_hex().to_string();
            map.entry(hex).or_default().push(short);
        }
    }

    Ok(map)
}
