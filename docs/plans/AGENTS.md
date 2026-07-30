# docs/plans/

## Responsibility

Project epic and story execution plans, roadmap blueprints, and task tracking files.

## Module Map

- **`Blueprint.md`** — High-level product summary, technical architecture, and macro roadmap goals.
- **`acp-context-policy/`**, **`acp-core-modularization/`**, **`acp-crate-extraction/`**, **`acp-session-history/`** — Subsystem epic plan directories.
- **`git-action-bar/`**, **`git-history-graph/`**, **`multi-client-acp-gateway/`** — Feature epic directories containing story breakdowns.
- **`other_tasks/`** — Minor bug fixes, chores, and standalone work items.
- **Status-Prefixed Files** — Top-level epic/story files named by status:
  - `pending-*.md` — Proposed or queued work items.
  - `active-*.md` — In-progress development tasks.
  - `done-*.md` — Completed work items kept for audit history.

## Rules & Patterns

- **Plan Management Skill**: Follow `.agents/skills/plan-management/SKILL.md` when creating or modifying plan files.
- **Status Prefixes**: Always prefix plan filenames with current status (`pending-`, `active-`, `done-`) and rename files when status changes.
- **Single-Branch Scope**: Keep plans concise and executable within a single git branch.
- **Task Review**: Task reviewers update file status or remove completed tasks after verification.
