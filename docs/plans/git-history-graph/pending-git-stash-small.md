# S-GIT-STASH — Stash support (optional)

> **Status:** Pending. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-action-bar-large.md`. **Optional.**

## Goal

Stash, pop, and list stashes from the git panel.

## Scope

- `POST /api/workspaces/{id}/git/stash` — `git stash push` with optional message.
- `POST /api/workspaces/{id}/git/stash/pop` — `git stash pop`.
- `GET /api/workspaces/{id}/git/stash/list` — list stash entries.
- Frontend: stash section in GitPanel (below the commit input), with
  stash/pop/drop actions per entry.

## Acceptance

- [ ] Stash creates a stash entry and cleans the working tree.
- [ ] Pop restores the stashed changes.
- [ ] List shows stash entries with messages.
- [ ] Drop removes a stash entry.
- [ ] `make check` passes.
