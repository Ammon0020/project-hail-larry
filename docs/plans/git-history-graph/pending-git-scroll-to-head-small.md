# S-GIT-SCROLL-TO-HEAD — Scroll to HEAD button

> **Status:** Pending. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-viewer-v2-medium.md`.

## Goal

Add a "Scroll to HEAD" button in the graph pane header that jumps the
virtualizer to the HEAD commit and briefly highlights it.

## Scope

- Find the HEAD commit (`isHead === true`) in the loaded commits window.
- Scroll the virtualizer to that row.
- If HEAD is not in the loaded window, fetch pages until found (or show a
  toast "HEAD not in recent history").
- Briefly highlight the HEAD row after scroll (e.g. a fading ring or
  background flash).

## Acceptance

- [ ] "Scroll to HEAD" button in the graph pane header scrolls to the HEAD
      row when HEAD is loaded.
- [ ] If HEAD is outside the loaded window, pages are fetched until HEAD is
      found or a "HEAD not in recent history" toast is shown.
- [ ] HEAD row is briefly highlighted after the scroll settles.
- [ ] `make check` passes.
