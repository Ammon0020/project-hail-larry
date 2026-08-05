# S-GIT-DETACHED-CHECKOUT — Detached HEAD checkout

> **Status:** Done (2026-07-30). **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-checkout-small.md`, `done-git-graph-viewer-v2-medium.md`.
>
> *Done 2026-07-30 — `git::checkout_commit` in `worktree.rs` runs `git checkout
> <sha>` with the same dirty-tree guard as branch checkout. New endpoint
> `POST /api/workspaces/{id}/git/checkout-commit` with `{ oid }`. Frontend
> `gitCheckoutCommit` API function. Context menu now shows "Checkout <branch>"
> for labeled commits and "Checkout <sha> (detached)" for unlabeled ones.
> GitPanel header shows "Detached HEAD" in amber when `headBranch` is null.
> 3 new Rust tests (not-a-repo, dirty-tree refusal, detached-head verification).
> `make qcheck` passes.*

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

- [x] `POST .../git/checkout-commit` checks out the given SHA into detached
      HEAD.
- [x] Dirty working tree returns 409 with a file list; UI shows a clear error.
- [x] Context menu Checkout works for commits with and without branch labels.
- [x] GitPanel header shows a "detached HEAD" indicator when HEAD is detached.
- [x] `make check` passes.
