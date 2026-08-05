# S-GIT-STASH — Stash support (optional)

> **Status:** Done (2026-07-30). **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-action-bar-large.md`.
>
> *Done 2026-07-30 — Backend: `stash_push`, `stash_pop`, `stash_drop`, `stash_list`
> in `worktree.rs` (shells out to `git` CLI; gix has no stash support). 4 REST
> routes registered. `StashEntry` wire type with index/oid/branch/message.
> `parse_stash_message` handles `WIP on` / `On` formats. 3 Rust tests (push/pop
> round-trip, drop, empty list). Frontend: `StashEntry` type + 4 API functions.
> GitPanel gains a collapsible Stashes section (only when stashes exist) with
> pop/drop per entry, and a Stash button next to Push. `runMutation` refreshes
> stashes after every mutation. 540 Rust tests + 72 vitest cases pass;
> `make qcheck` passes.*

## Goal

Stash, pop, and list stashes from the git panel.

## Scope

- `POST /api/workspaces/{id}/git/stash` — `git stash push` with optional message.
- `POST /api/workspaces/{id}/git/stash/pop` — `git stash pop`.
- `GET /api/workspaces/{id}/git/stash/list` — list stash entries.
- Frontend: stash section in GitPanel (below the commit input), with
  stash/pop/drop actions per entry.

## Acceptance

- [x] Stash creates a stash entry and cleans the working tree.
- [x] Pop restores the stashed changes.
- [x] List shows stash entries with messages.
- [x] Drop removes a stash entry.
- [x] `make check` passes.
