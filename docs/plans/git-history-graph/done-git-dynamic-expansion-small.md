# S-GIT-DYNAMIC-EXPANSION — Dynamic inline expansion height

> **Status:** Done (2026-07-30). **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-viewer-v2-medium.md`.
>
> *Done 2026-07-30 — `GitHistorySection.tsx` now uses a `Set<string>` for
> multiple simultaneous expansions and a `flatItems` array that interleaves
> commits and slots. Slot height is measured dynamically via
> `virtualizer.measureElement` with `data-index`, auto-re-measuring when the
> file list loads. `CommitFileList.tsx` replaces the fixed `max-h-40` with a
> `maxHeight` prop (default 400px). Continuation verticals scan forward to the
> next commit, handling consecutive expansions correctly. 72 vitest cases pass;
> `make qcheck` passes.*

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

- [x] Expanded rows size to their actual content height (no fixed 160px slot).
- [x] Height re-measures when the file list finishes loading.
- [x] Multiple rows can be expanded at once without layout breakage.
- [x] `make check` passes.
