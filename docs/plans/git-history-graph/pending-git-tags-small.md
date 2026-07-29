# S-GIT-TAGS — Tag display in graph (optional)

> **Status:** Pending. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** S-GIT-LOG-API, S-GIT-GRAPH-VIEWER. **Optional.**

## Goal

Display tag labels alongside branch labels in the commit graph.

## Scope

- Extend S-GIT-LOG-API to include tag labels in `branch_labels` (or a separate
  `tag_labels` field).
- Render tags with a distinct chip color in the graph.
- No tag creation for v1 (display only).

## Acceptance

- [ ] Tags appear as chips in the graph with a distinct color.
- [ ] `make check` passes.
