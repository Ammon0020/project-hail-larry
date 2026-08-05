# S-GIT-TAGS — Tag display in graph (optional)

> **Status:** Done (2026-07-30). **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** S-GIT-LOG-API, S-GIT-GRAPH-VIEWER.
>
> *Done 2026-07-30 — `LogCommit` gains `tag_labels: Vec<String>`. `build_tag_refs`
> in `history.rs` scans `refs/tags/*`, peels annotated tags to their target
> commit, and maps commit oid → short tag names. Tags don't seed the rev_walk.
* Frontend renders amber tag chips after branch chips in `GraphRow`. 2 Rust tests
> (lightweight + annotated tag). 72 vitest cases pass; `make qcheck` passes.*

## Goal

Display tag labels alongside branch labels in the commit graph.

## Scope

- Extend S-GIT-LOG-API to include tag labels in `branch_labels` (or a separate
  `tag_labels` field).
- Render tags with a distinct chip color in the graph.
- No tag creation for v1 (display only).

## Acceptance

- [x] Tags appear as chips in the graph with a distinct color.
- [x] `make check` passes.
