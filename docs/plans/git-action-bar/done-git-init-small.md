# S-GIT-INIT — Initialize git in workspace

> Story. Difficulty: small. Urgency: medium. Epic: `pending-git-action-bar-large.md`.
> Depends on: S-GIT-DETECT.

## Goal

When a workspace has no git repo, let the user initialize one from the UI
instead of dropping to a terminal. Reuses the git-detection infrastructure
from S-GIT-DETECT.

## Scope

- `POST /api/workspaces/{id}/git/init` — authenticated paired-device gate
  (same model as file-save/rename/mkdir; no permission sink). Implementation
  in `src/git/ops.rs::init` using `gix` to create the repo (no `git` CLI
  spawn). Returns `{ oid: string }` (initial empty-tree oid).
  - Refuse if a `.git` already exists at the workspace root (400).
  - Reject if the workspace root is a symlink (existing workspace policy).
  - Default branch name: `main` (configurable via `config.toml` later;
    hardcoded for MVP).
- Frontend: when `useGitState` reports `repo_detected === false`, render a
  small "Initialize git" affordance in `WorkspaceHeader.tsx` (or Settings →
  Workspace, once that section exists). Confirmation dialog explains that
  this creates `.git/` at the workspace root and is reversible only by
  deleting that directory.
- After successful init, invalidate the detection cache (S-GIT-DETECT) so the
  action bar item appears on the next render.

## Out of scope

- `.gitignore` templating (future story; user can add one manually after
  init).
- Initial commit creation (user follows up via S-GIT-ACTION-BAR).
- Choosing a non-`main` default branch from the UI (config-only for MVP).

## Acceptance

- [ ] `POST /api/workspaces/{id}/git/init` creates a repo and returns the
      initial oid; 400 if `.git` exists; 401 unauthenticated; authenticated
      paired-device gate (no permission prompt).
- [ ] UI affordance appears only when `repo_detected === false`; confirmation
      dialog explains the side effect.
- [ ] After init, the action bar item and breadcrumb branch appear without a
      page reload (cache invalidation).
- [ ] Refuses symlinked workspace roots.
- [ ] Unit + contract test for the endpoint; component test for the
      affordance + confirmation flow.

## Verification

- `cargo test -q --all-targets` with a temp-dir fixture.
- New contract fixture for `/git/init`.
- `make check` passes.

Suggested commit: `feat(git): initialize git in workspace (S-GIT-INIT)`
