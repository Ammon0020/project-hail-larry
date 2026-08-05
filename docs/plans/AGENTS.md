# docs/plans/

## Responsibility

Project epic and story execution plans, roadmap blueprints, and task tracking files.

## Module Map

```text
docs/plans/
├── Blueprint.md
├── acp-context-policy/       context-policy stories
├── acp-core-modularization/ ACP core stories
├── acp-crate-extraction/    crate-boundary stories
├── acp-session-history/     history stories
├── multi-client-acp-gateway gateway stories
└── other_tasks/             bugs/chores/standalone work
    └── {pending,active,done}-*.md
```

## Rules & Patterns

- **Plan Management Skill**: Follow `.agents/skills/plan-management/SKILL.md` when creating or modifying plan files.
- **Status Prefixes**: Always prefix plan filenames with current status (`pending-`, `active-`, `done-`) and rename files when status changes.
- **Single-Branch Scope**: Keep plans concise and executable within a single git branch.
- **Task Review**: Task reviewers update file status or remove completed tasks after verification.
