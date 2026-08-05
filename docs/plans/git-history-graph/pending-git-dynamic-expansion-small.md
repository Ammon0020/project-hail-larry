# S-GIT-DYNAMIC-EXPANSION — Dynamic inline expansion height

> **Status:** Pending. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-viewer-v2-medium.md`.

## Goal

Replace the fixed 160px inline expansion slot with dynamic measurement so the
expanded file list sizes to its actual content and supports multiple
simultaneous expansions.

## Scope

- Use `virtualizer.measureElement(el)` after expansion to measure actual
  content height, or render the file list as an overlay portal positioned
  below the clicked row.
- Handle re-measurement when the file list finishes loading (height changes
  from spinner to content).
- Support multiple simultaneous expansions.

## Acceptance

- [ ] Expanded rows size to their actual content height (no fixed 160px slot).
- [ ] Height re-measures when the file list finishes loading.
- [ ] Multiple rows can be expanded at once without layout breakage.
- [ ] `make check` passes.
