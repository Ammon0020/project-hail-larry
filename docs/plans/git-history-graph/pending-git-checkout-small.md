# S-GIT-CHECKOUT — Branch switching

> **Status:** Pending. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-action-bar-large.md` (git panel surface).

## Goal

Switch branches from the UI via `POST /api/workspaces/{id}/git/checkout`.

## Scope

- `POST /api/workspaces/{id}/git/checkout` with `{ branch: string }`.
- Backend: `git checkout <branch>` via CLI (gix doesn't support checkout write
  ops). Refuses if working tree dirty (409 + file list).
- Frontend: branch dropdown in GitPanel header (replaces the static branch
  display). Shows local branches, highlights current. On select → checkout →
  refresh status + file tree.
- No remote branch creation, no new branch creation for v1.

## Acceptance

- [ ] Branch dropdown shows all local branches, highlights current.
- [ ] Selecting a branch checks it out and refreshes status + file tree.
- [ ] Dirty working tree returns 409 with file list; UI shows a clear error.
- [ ] `make check` passes.
