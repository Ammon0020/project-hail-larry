# S-GIT-CHECKOUT — Branch switching

> **Status:** Done. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-action-bar-large.md` (git panel surface).

## Goal

Switch branches from the UI via `POST /api/workspaces/{id}/git/checkout`.

## Scope

- `POST /api/workspaces/{id}/git/checkout` with `{ branch: string }`.
- Backend: `git checkout <branch>` via CLI (gix doesn't support checkout write
  ops). Refuses if working tree dirty (409 + file list).
- Frontend: searchable branch-picker modal in the GitPanel header (replaces the
  static branch display and inline dropdown). Shows local and remote-tracking
  branches, highlights current, and supports click or keyboard selection. On
  select → checkout → refresh status + file tree.
- No explicit remote branch creation or detached-head actions for v1.
- `status()` returns `branches: Vec<String>` containing local and
  remote-tracking short names so the frontend can populate the picker without a
  separate endpoint.

## Acceptance

- [x] Branch dropdown shows all local branches, highlights current.
- [x] Selecting a branch checks it out and refreshes status + file tree.
- [x] Dirty working tree returns 409 with file list; UI shows a clear error.
- [x] `make check` passes.

## Implementation notes

- **`branches` in `StatusResult`** (`src/git/types.rs`, `src/git/repo.rs`):
  Added `branches: Vec<String>` field, populated via
  `git branch --format=%(refname:short)`. One clean name per line, no `*`
  marker parsing. Errors are swallowed (empty vec) so status never breaks.
- **`checkout()`** (`src/git/worktree.rs`): Follows the `pull()` pattern —
  refuses dirty trees with `GitError::DirtyTree` (409), shells out to
  `git checkout <branch>`, returns stderr.
- **Route** (`src/api/git.rs`, `src/api/mod.rs`): `POST .../git/checkout`
  handler with `CheckoutRequest { branch: String }`. Empty branch → 400.
- **Frontend** (`web/src/lib/api/git.ts`, `web/src/components/git/`):
  `gitCheckout` API function; the static branch display replaced with a
  `BranchPicker` Radix dialog. The picker auto-focuses its search input,
  filters local and remote entries case-insensitively, displays remote names as
  `origin/branch` with a muted prefix, supports arrow-key navigation and Enter,
  and marks the current branch with a check. It also shows persistent stub rows
  for `Create new branch...`, `Create new branch from...`, and
  `Checkout detached...`, each marked `Coming soon` and excluded from branch
  search/navigation. On select, it calls `gitCheckout` via
  `runMutation('checkout', ...)` then `onRepoChanged()` to refresh the file
  tree.
- **Tests** (`src/git/tests.rs`): 6 backend tests cover branch listing
  (single, multiple, remote refs with `origin/HEAD` filtering), checkout
  not-a-repo, checkout dirty-tree refusal, and checkout switching the active
  branch. The frontend intentionally has no React/DOM tests; its existing
  Vitest suite remains pure-function-only and no meaningful standalone helper
  was introduced by the picker.
