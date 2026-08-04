# S-GIT-FETCH-PULL — Fetch and pull from remote

> **Status:** Done. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-action-bar-large.md` (push handler + credential model).

## Goal

Fetch and pull from the remote, completing the remote sync loop.

## Scope

- `POST /api/workspaces/{id}/git/fetch` — `git fetch`, streams stderr.
- `POST /api/workspaces/{id}/git/pull` — `git pull`, streams stderr, refuses if
  dirty (409).
- Frontend: "Fetch" button in GitPanel header. Pull is a dropdown option in the
  "More actions" menu.
- Update `status()` to populate `upstream`/`ahead`/`behind` (previously returned
  zeros by design — now uses `git rev-parse @{u}` + `git rev-list --left-right
  --count`).
- Same credential model as push (agent environment, no storage).

## Acceptance

- [x] Fetch works and streams stderr to the UI.
- [x] Pull works, refuses if dirty (409), streams stderr.
- [x] `status()` returns real `upstream`/`ahead`/`behind` values.
- [x] Ahead/behind chips update after fetch/pull.
- [x] `make check` passes.

## Implementation notes

- **`status()` upstream/ahead/behind** (`src/git/repo.rs`): Added
  `upstream_ahead_behind()` helper that shells out to
  `git rev-parse --abbrev-ref --symbolic-full-name @{u}` for the upstream name
  and `git rev-list --left-right --count <upstream>...HEAD` for the counts.
  Errors are swallowed (returns `(None, 0, 0)`) so repos without upstreams
  still work.
- **`fetch()` / `pull()`** (`src/git/worktree.rs`): Follow the `push()` pattern.
  `pull()` checks `status()` first and returns `GitError::DirtyTree` (mapped to
  409 CONFLICT) if the working tree has uncommitted changes.
- **`GitError::DirtyTree`** (`src/git/types.rs`): New error variant mapped to
  `AppError::conflict` (409).
- **Routes** (`src/api/git.rs`, `src/api/mod.rs`): `POST .../git/fetch` and
  `POST .../git/pull` handlers using the shared `run_git_blocking` helper.
- **Frontend** (`web/src/lib/api/git.ts`, `web/src/components/git/GitPanel.tsx`):
  `gitFetch`/`gitPull` API functions; Fetch button (DownloadCloud icon) in the
  GitPanel header; "Pull from remote" item in the More actions dropdown
  (GitPullRequest icon). After pull, `onRepoChanged()` refreshes the file tree.
- **Unit tests** (`src/git/tests.rs`): 6 new tests covering upstream resolution
  (ahead, behind, no-tracking), fetch/pull not-a-repo, and pull dirty-tree
  refusal. The upstream test helper registers a dummy `origin` remote because
  `git rev-parse @{u}` validates the remote config, not just the remote-tracking
  ref.
