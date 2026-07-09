# Re-adding a workspace ID (or nested roots) leaves stale watches

- **Difficulty:** easy
- **Urgency:** low
- **File:** `internal/fswatch/watcher.go`
- **Lines:** 88-93, 115-129

## Description

`AddWorkspace` unconditionally overwrites `w.roots[id]` with the new path without removing the watches for the previous path. If the same ID is re-registered with a different root (or a root nested under another workspace is removed), `RemoveWorkspace` only removes watches under the *current* `root`, leaving the old root's watches in place forever. Also, removing a workspace whose root is nested inside another still-watched workspace is fine, but removing the *outer* workspace removes watches the inner one still needs (prefix match is path-based, not reference-counted).

## Recommendation

In `AddWorkspace`, if `id` already exists with a different path, call `RemoveWorkspace(id)` first. For shared subtrees, reference-count watched paths by how many roots cover them, or accept the limitation and document it.

## Verification

Read `watcher.go` 88-93: `w.roots[id] = absPath` with no check of prior value. Read 115-129: removal only walks `WatchList` against the single stored `root`.
