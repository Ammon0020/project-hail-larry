use std::path::Path;

use gix::bstr::ByteSlice;

use super::repo::open_repo;
use super::{CommitAuthor, GitError, LogCommit, LogResult};

/// Maximum number of commits returned by [`log`] in a single response.
/// Matches the story spec's cap; higher `limit` values are clamped to this.
pub const MAX_LOG_LIMIT: u32 = 200;

/// `GET /api/workspaces/{id}/git/log?limit=100&offset=0` (S-GIT-LOG-API).
///
/// Walks the commit graph from every local branch head (plus the checked-out
/// HEAD, in case it is detached) using `gix` (no `git log` CLI spawn),
/// returning a paginated list with parent refs, branch labels, and the HEAD
/// marker. An unborn repo (no commits) returns an empty list, not an error.
///
/// Walking the union of branch heads — not just `HEAD` — means a diverged
/// local branch's unique commits appear in the log even when it isn't
/// checked out, so the frontend can render the full branch topology. The
/// `is_head` flag is still set only on the commit `HEAD` actually points at.
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

    // Branch labels and walk tips: scan local refs once and build both the
    // commit oid → short branch names map (for per-commit labels) and the set
    // of branch-head oids to seed the union walk. Cheap; refs are a small list.
    let (branch_map, mut tips) = build_branch_refs(&repo)?;

    // Seed the walk with HEAD too, so a detached HEAD's commit (which no local
    // branch points at) still appears and `is_head` always has a match. When
    // HEAD sits on a branch the oid is already in `tips`; dedup keeps it cheap.
    if !tips.contains(&head_oid) {
        tips.push(head_oid);
    }

    // Walk all commits reachable from the union of branch heads + HEAD,
    // topological breadth-first. We collect the full walk into a Vec so we can
    // report `total` for pagination — repos with huge histories may want a
    // streaming approach later, but for the MVP this is simple and correct.
    let walk = repo
        .rev_walk(tips)
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

/// Build the branch label map and the set of local branch-head oids used to
/// seed the union walk.
///
/// Scans `refs/heads/*` once and peels each to its target commit, returning:
/// - a map of commit hex oid → short branch names (multiple branches can point
///   at the same commit, e.g. after a fast-forward, so the value is a `Vec`);
/// - the detached oids of every local branch head, for `rev_walk` tips.
///
/// Errors are non-fatal: a broken ref is skipped rather than failing the whole
/// log call.
/// Commit hex oid → short local branch names pointing at it.
type BranchLabelMap = std::collections::HashMap<String, Vec<String>>;

fn build_branch_refs(
    repo: &gix::Repository,
) -> Result<(BranchLabelMap, Vec<gix::ObjectId>), GitError> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut tips: Vec<gix::ObjectId> = Vec::new();
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
        // a branch that doesn't peel to a commit is skipped. The detached oid
        // seeds the union walk; its hex string keys the label map.
        let mut branch = branch;
        if let Ok(id) = branch.peel_to_id() {
            let oid = id.detach();
            let hex = oid.to_hex().to_string();
            map.entry(hex).or_default().push(short);
            if !tips.contains(&oid) {
                tips.push(oid);
            }
        }
    }

    Ok((map, tips))
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
