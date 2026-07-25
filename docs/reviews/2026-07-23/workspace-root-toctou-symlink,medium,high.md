# TOCTOU — workspace root replaced by a symlink after registration escapes containment

- **Difficulty:** medium
- **Urgency:** high
- **File:** `src/workspace/mod.rs`, `src/pathutil/mod.rs`
- **Lines:** workspace/mod.rs 115-135 (root_for / safe_path), pathutil/mod.rs 70-108 (clean_path, esp. line 80)

## Description

The root is canonicalized **once** at registration (workspace/mod.rs:164) and stored. `root_for` (lines 115-130) returns the stored path with no re-validation. `safe_path` calls `clean_path`, which at pathutil/mod.rs:80 does `fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf())` on **every** call — i.e. it re-resolves the root at access time. If, after registration, an attacker (or a shell command approved earlier) replaces the workspace root with a symlink to `/etc` (`mv proj proj.real && ln -s /etc proj`), then a subsequent `read_file(ws, "passwd")` canonicalizes the root to `/etc`, joins `passwd` → `/etc/passwd`, and `is_within_root("/etc", "/etc/passwd")` is **true**. The file is served. `resolve_symlink` only inspects the *final component* of the requested path (`passwd`, a regular file), not the root, so it does not catch this. The registration-time symlink check (which is itself missing — see the workspace-root-symlink-accepted finding) is not re-run. This is a classic TOCTOU that defeats the containment invariant.

## Recommendation

Pin the workspace root at registration: store the canonicalized path AND its `symlink_metadata`-verified non-symlink status, and on every `safe_path`/`root_for` call re-check `symlink_metadata(root).is_symlink()` is false and that `fs::canonicalize(root)` still equals the stored canonical root. If either changes, mark the workspace unavailable (return the existing `AppError::validation("workspace unavailable: ...")` path) until re-registration. Alternatively, open a file descriptor on the root at registration and do all path resolution relative to that fd (`openat(2)`-style) so a later symlink swap cannot redirect it.

## Verification

root_for (lines 115-130) clones `entry.path` with no disk re-check. clean_path (pathutil/mod.rs:80) re-canonicalizes the root on each call. resolve_symlink (pathutil/mod.rs:133-205) checks `symlink_metadata(path)` for the *requested* path's final component (line 135) and parent (line 170), but never the root. No code path compares the live root against the stored canonical root.
