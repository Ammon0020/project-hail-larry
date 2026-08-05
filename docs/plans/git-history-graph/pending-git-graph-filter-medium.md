# S-GIT-GRAPH-FILTER — Filter/search git history

> **Status:** Pending. **Difficulty:** medium. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-viewer-v2-medium.md`.

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

- [ ] Search bar in the graph pane header filters commits as you type.
- [ ] Filter matches author name/email, message substring, and SHA prefix.
- [ ] "X of Y commits" count shows while a filter is active.
- [ ] Input is debounced (~200ms); clearing the filter restores the full list.
- [ ] `make check` passes.
