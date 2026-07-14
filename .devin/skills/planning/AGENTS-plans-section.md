# Planning System

All plans live in `plans/`. Structure encodes progress — listing the folder shows where the project stands. The `/planning` skill is for creating and maintaining plan structure, not for implementation work.

## Layout

```
plans/
├── app-vision.md            # Vision + phase roadmap. Read first. Changes only on user direction.
├── P<n>-<epic>.md           # One per epic. Phase prefix + kebab-case. High-level scope + Status section.
├── P<n>-<epic>/             # Story folder for that epic.
│   └── <status>-<desc>-<urgency>-<difficulty>.md
└── maintenance/             # Standalone stories (bugs, fixes, refactors) not tied to an epic.
```

## Story filenames

`<status>-<description>-<urgency>-<difficulty>.md`

- **Status**: `pending` · `wip` · `done` · `blocked` — renamed as work progresses.

Listing a story folder shows status, urgency, and difficulty at a glance.

## Epic Status section

Each epic ends with a Status section, one item per feature: `[indicator] brief — file refs`.

Indicators: `✅ done` · `🔄 wip` · `⬜ pending` · `❌ blocked`.

Update it as features complete or block. When all items are `✅` (or the epic is cancelled), the epic file and story folder are deleted — plans show what needs doing, not what was done.

## Dependencies

- **Epics** — optional `Depends on: P1-foundation` line near the top.
- **Stories** — optional `Depends on: <story-filename>` line, can point across folders (incl. `maintenance/`).

Don't start a story whose dependency isn't `done` without flagging it to the user.

## Implementor workflow

1. Read `app-vision.md` for context, then the relevant epic for scope and status.
2. List the epic's story folder — filenames show status/urgency/difficulty without opening files.
3. Check `Depends on:` lines on candidates before picking.
4. Pick: prefer `wip` (resume), then `pending` by urgency (high → med → low). Respect phase ordering — don't start a P2 epic if its P1 dependency isn't done.
5. **Start:** rename the story to `wip-…` (`git mv` for tracked files). Read the story file for goal, acceptance criteria, and file refs.
6. **Blocked:** rename to `blocked-…`, note the blocker in the file, mark the epic Status item `❌`.
7. **Done:** rename to `done-…`, mark the epic Status item `✅` with file refs. Check whether any `blocked` stories were waiting on this one.
8. Update story files with file references you discover as you work.

## Story file contents

Goal, acceptance criteria, file references, optional `Depends on:`. Kept short — implementation detail goes in the code, not the story.

## Rules

- Phases (`P1`, `P2`) for coarse sequencing, not strict per-epic numbering. Multiple epics can share a phase.
