# Planning System Changelog

High-level changes to the planning system (skill, AGENTS section, conventions). Under 100 lines. Dates are YYYY-MM-DD.

## 2026-07-12

### Added
- Planning skill (`.devin/skills/planning/SKILL.md`) — planner-facing: creating epics/stories, phase assignment, dependencies, pruning.
- AGENTS section (`.devin/skills/planning/AGENTS-plans-section.md`) — implementor-facing: layout, filename format, Status indicators, workflow, rules. Self-contained; implementors don't read the skill.
- **Phases** — epics carry a phase prefix (`P1`, `P2`, …). Coarse sequencing; multiple epics can share a phase. Replaces strict per-epic numbering. Roadmap lives in `app-vision.md`.
- **`Depends on:` lines** — optional, in epic and story files. Handles DAG ordering phases can't: epic-to-epic, story-to-story, cross-folder (incl. `maintenance/`).
- **Cancelled epic handling** — cancelled epics are deleted (like completed ones), with a one-line note in `app-vision.md`.
- This changelog.

### Decisions
- **Phases over numbering.** Per-epic sequential numbers imply a total order, go stale on insert, and can't express parallel epics. Phases are coarser, shared, and editable in one roadmap file.
- **`Depends on:` over filename encoding.** Dependencies aren't visible from `ls`, but a `Depends on:` line survives reordering and can express a DAG. The phase roadmap covers bird's-eye sequencing.
- **Split skill vs AGENTS section.** Implementors see the AGENTS section every chat and shouldn't need the skill. The skill is for planners creating/maintaining structure.
- **Stories inherit epic phase.** No phase prefix on story filenames — keeps them clean, phase is visible from the folder name.
- **Description field uses `snake_case`.** Hyphens are field separators in the filename; mixing them into descriptions creates ambiguity.

### Removed
- Subagent model-tier strategy (from source material) — model names change fast, not part of the planning system.
