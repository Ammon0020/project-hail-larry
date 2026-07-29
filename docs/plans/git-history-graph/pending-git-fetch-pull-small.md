# S-GIT-FETCH-PULL — Fetch and pull from remote

> **Status:** Pending. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-action-bar-large.md` (push handler + credential model).

## Goal

Fetch and pull from the remote, completing the remote sync loop.

## Scope

- `POST /api/workspaces/{id}/git/fetch` — `git fetch`, streams stderr.
- `POST /api/workspaces/{id}/git/pull` — `git pull`, streams stderr, refuses if
  dirty (409).
- Frontend: "Fetch" button in GitPanel header. Pull is a dropdown option on the
  fetch button (or a separate button if ahead/behind chips indicate behind).
- Update `status()` to populate `upstream`/`ahead`/`behind` (currently returns
  zeros by design — this story implements the reference traversal).
- Same credential model as push (agent environment, no storage).

## Acceptance

- [ ] Fetch works and streams stderr to the UI.
- [ ] Pull works, refuses if dirty (409), streams stderr.
- [ ] `status()` returns real `upstream`/`ahead`/`behind` values.
- [ ] Ahead/behind chips update after fetch/pull.
- [ ] `make check` passes.
