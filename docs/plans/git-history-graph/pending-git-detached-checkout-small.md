# S-GIT-DETACHED-CHECKOUT — Detached HEAD checkout

> **Status:** Pending. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-checkout-small.md`, `done-git-graph-viewer-v2-medium.md`.

## Goal

Let the context menu Checkout action work for any commit, not just those with
branch labels, by checking out the commit's SHA into a detached HEAD.

## Scope

- New backend endpoint `POST /api/workspaces/{id}/git/checkout-commit` with
  `{ oid: string }` that runs `git checkout <sha>` (detached HEAD).
- Refuse if the working tree is dirty (409 + file list), same as branch
  checkout.
- Frontend: context menu Checkout is enabled for all commits; for commits
  without branch labels it calls the new endpoint, for labeled commits it
  reuses the existing branch checkout.
- Show a "detached HEAD" indicator in the GitPanel header when HEAD is
  detached.

## Acceptance

- [ ] `POST .../git/checkout-commit` checks out the given SHA into detached
      HEAD.
- [ ] Dirty working tree returns 409 with a file list; UI shows a clear error.
- [ ] Context menu Checkout works for commits with and without branch labels.
- [ ] GitPanel header shows a "detached HEAD" indicator when HEAD is detached.
- [ ] `make check` passes.
