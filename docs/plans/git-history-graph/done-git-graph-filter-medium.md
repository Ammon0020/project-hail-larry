# S-GIT-GRAPH-FILTER — Filter/search git history

> **Status:** Done (2026-07-30). **Difficulty:** medium. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-viewer-v2-medium.md`.
>
> *Done 2026-07-30 — Client-side filter in `GitHistorySection.tsx`. Debounced
> (200ms) input in the graph pane header matches author name/email, message
> substring, and SHA prefix (case-insensitive). Shows "X/Y" count when filtered.
> `filteredCommits` feeds `flatItems`, `layoutGitGraph`, and
> `commitIndexToVirtualIndex`; `loadMore` and `refresh` use the full `commits`
> array. `pendingScrollHead` switched from index to oid so scroll-to-HEAD
> resolves against the filtered list, with a "HEAD hidden by filter" notice.
> "No commits match" empty state for zero results. 72 vitest cases pass;
> `make qcheck` passes.*

## Goal

Add a search bar to the graph pane header that filters the visible commits by
author, message substring, or SHA prefix.

## Scope

- Client-side filter on the loaded commits window (no backend change for v1).
- Match against author name, author email, commit message substring, and SHA
  prefix (case-insensitive).
- Show "X of Y commits" count when filtered.
- Debounced input (200ms).
- Clear-filter button.
- Future: backend-side filter via query params on `/git/log` for large repos.

## Acceptance

- [x] Search bar in the graph pane header filters commits as you type.
- [x] Filter matches author name/email, message substring, and SHA prefix.
- [x] "X of Y commits" count shows while a filter is active.
- [x] Input is debounced (~200ms); clearing the filter restores the full list.
- [x] `make check` passes.
