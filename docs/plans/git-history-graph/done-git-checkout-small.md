# S-GIT-CHECKOUT — Branch switching

> **Status:** Done. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
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
- `status()` now also returns `branches: Vec<String>` (local branch short names)
  so the frontend can populate the dropdown without a separate endpoint.

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
- **Frontend** (`web/src/lib/api/git.ts`, `web/src/components/git/GitPanel.tsx`):
  `gitCheckout` API function; the static branch `<span>` replaced with a
  `DropdownMenu` trigger (branch icon + name + chevron, chevron only when >1
  branch). Current branch item is disabled with a `Check` icon. On select,
  calls `gitCheckout` via `runMutation('checkout', ...)` then `onRepoChanged()`
  to refresh the file tree.
- **Unit tests** (`src/git/tests.rs`): 5 new tests covering branch listing
  (single, multiple), checkout not-a-repo, checkout dirty-tree refusal, and
  checkout switching the active branch.
