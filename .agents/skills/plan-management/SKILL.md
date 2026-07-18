---
name: plan-management
description: Create, discover, update, and complete this project's epic and story plans using status-visible filenames. Use when planning work, choosing the next epic or story, updating plan status, or working under docs/plans/.
---

# Plan Management

Make work status visible from directory listings; reserve plan contents for scope, dependencies, acceptance criteria, and verification.

## Layout

```
docs/plans/
├── status-epic-difficulty.md          # epic files at top level
├── epic/                              # folder named after the epic
│   └── status-story-difficulty.md     # stories inside
└── other_tasks/                       # bugs, chores, etc.
    └── status-task-difficulty-urgency.md
```

Use `pending`, `active`, `blocked`, or `complete` as the status prefix. Epic
and story filenames carry difficulty; non-epic tasks in `other_tasks/` carry
both difficulty and urgency (stories inherit urgency from their epic).

## Discover work

1. List `docs/plans/` before opening plans; status-prefixed epic files reveal the portfolio state.
2. List the chosen epic folder to find its status-prefixed stories; read only the story being worked and its epic summary.
3. Use `docs/STATUS.md` for the implementation snapshot and `OpenItems.md` for unresolved decisions. Do not treat reference/design documents as work items.

## Name and update work

- Epics: `<status>-<epic>-<difficulty>.md` at the top level of `docs/plans/`, with a sibling `<epic>/` folder holding its stories.
- Stories: `<status>-<story>-<difficulty>.md` inside the epic's folder.
- Non-epic tasks (bugs, chores): `<status>-<task>-<difficulty>-<urgency>.md` inside `docs/plans/other_tasks/`.
- On a status change, rename or move the existing plan; do not copy it or leave a stale status in its filename. Update links, the epic summary, and `docs/STATUS.md` in the same change.
- Keep plans concise and executable in one branch/work item. Record deferred gaps in `docs/known-issues.md`, not by silently expanding a story.
