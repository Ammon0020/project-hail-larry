---
name: planning
description: Create and maintain the plans/ folder — epics, phases, stories, and pruning. Use when creating plans, assigning phases, setting up dependencies, or cleaning up completed/cancelled work. Not for implementation work.
argument-hint: "[epic or story path, or empty to survey all plans]"
allowed-tools:
  - read
  - grep
  - glob
  - edit
  - write
  - exec
  - find_file_by_name
  - todo_write
  - ask_user_question
---

# Planning System (for planners)

You maintain the `plans/` folder structure. Implementors read the AGENTS.md "Planning System" section for their workflow — you don't need to repeat it. Your job: create epics and stories, assign phases, wire dependencies, prune completed/cancelled work, and keep `app-vision.md`'s phase roadmap honest.

## Naming conventions

- **Epics**: `P<n>-<epic-name>.md` — phase prefix + kebab-case. No per-epic sequential numbering; phases are coarse and shared.
- **Stories**: `<status>-<desc>-<urgency>-<difficulty>.md`. Description is `snake_case` (hyphens are field separators). Status/urgency/difficulty values are fixed: `pending|wip|done|blocked` · `high|med|low` · `easy|med|hard`.
- **Story folders**: same name as the epic file. Stories inherit their epic's phase — no phase prefix on story filenames.

## Creating an epic

1. Assign a phase in consultation with the user — check `app-vision.md` for the existing roadmap.
2. Create `plans/P<n>-<epic>.md` with: goal, scope, architecture decisions, optional `Depends on: <epic>` line, and a Status section listing features to implement (all `⬜ pending` initially).
3. Create the story folder `plans/P<n>-<epic>/`.
4. Add the epic to the phase roadmap in `app-vision.md`.

## Creating a story

1. Create in `plans/P<n>-<epic>/` (or `plans/maintenance/` for standalone work).
2. Filename: `pending-<desc>-<urgency>-<difficulty>.md`.
3. Contents: goal, acceptance criteria (required — no criteria, no story), file references, optional `Depends on: <story-file>` (can point across folders, incl. `maintenance/`).
4. Keep it short — implementation detail goes in the code.

## Pruning

- **Completed stories**: delete when no longer needed for reference.
- **Completed epic**: when all Status items are `✅`, delete the epic file and its story folder. Plans show what needs doing, not what was done.
- **Cancelled epic**: delete the epic file and story folder, and add a one-line note to `app-vision.md` so the roadmap stays honest.
- Update `app-vision.md`'s phase roadmap when epics are added, completed, or cancelled.

## Anti-patterns

- Per-epic sequential numbering (use phases).
- Hyphens in the description field (use `snake_case`).
- Stories without acceptance criteria.
- Stale Status sections — update on every completion/block.
- Keeping completed or cancelled epics around.
- Ignoring `Depends on:` when sequencing work.

## Ask the user before

- Creating a new epic (scope + phase assignment).
- Changing an epic's phase (rename file + update `app-vision.md`).
- Deleting an epic file and its story folder.
- Editing `app-vision.md` beyond roadmap maintenance.
