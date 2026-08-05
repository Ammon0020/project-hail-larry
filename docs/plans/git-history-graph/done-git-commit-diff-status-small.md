# S-GIT-COMMIT-DIFF-STATUS — Commit diff file status codes

> **Status:** Done. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-log-api-medium.md`, `done-git-graph-viewer-medium.md`.

## Goal

Surface per-file status codes (A/M/D/R) on commit diffs so the graph's inline
expansion can show what happened to each file.

## Scope

- Backend `commit_diff()` uses `git diff-tree --name-status -r -z -M` and parses
  A/M/D/R records.
- `CommitDiffFile` gains `status: char` and `old_path: Option<String>` fields
  (R records carry the source path).
- Frontend `CommitDiffFile` type mirrors with `status: CommitFileStatus` and
  `oldPath: string | null`.
- Status badges consumed by the graph viewer v2 inline expansion
  (`CommitStatusBadge.tsx`).

## Acceptance

- [x] `commit_diff()` returns `status` and `old_path` for every file.
- [x] Add / modify / delete / rename records parse correctly.
- [x] Frontend type mirrors the backend wire shape.
- [x] `commit_diff_name_status_parses_add_modify_delete_rename` passes in
      `src/git/tests.rs`.
- [x] `make check` passes.

## Status note

Done in this wave. Backend + frontend types landed together; the graph viewer
v2 story consumes the status field for inline expansion badges.
