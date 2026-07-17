---
name: plan-management
description: Create, discover, update, and complete this project's epic and story plans using status-visible filenames. Use when planning work, choosing the next epic or story, updating plan status, or working under docs/plans/.
---

# Plan Management

Make work status visible from directory listings; reserve plan contents for
scope, dependencies, acceptance criteria, and verification.

## Discover work

1. List `docs/plans/` before opening plans; status-prefixed epic folders reveal
   the portfolio state.
2. List the chosen epic folder to find its status-prefixed stories; read only
   the story being worked and its epic summary.
3. Use `docs/STATUS.md` for the implementation snapshot and `OpenItems.md` for
   unresolved decisions. Do not treat reference/design documents as work items.

## Name and update work

- Name work epics `<status>-<epic>/` and stories
  `<status>-<story>-<urgency>-<difficulty>.md`. Use `pending`, `active`,
  `blocked`, or `complete` as the status.
- Keep `epic.md` inside its epic folder. Put stories directly in that folder or
  in its `stories/` subfolder; use one layout consistently within an epic.
- On a status change, rename or move the existing plan; do not copy it or leave
  a stale status in its filename. Update links, the epic summary, and
  `docs/STATUS.md` in the same change.
- Keep plans concise and executable in one branch/work item. Record deferred
  gaps in `docs/known-issues.md`, not by silently expanding a story.
