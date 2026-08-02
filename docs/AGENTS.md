# docs/

## Responsibility

Project documentation, plans, specs, reference, and review findings.

## Module Map

```text
docs/
├── STATUS.md, known-issues.md   status/deferred gaps
├── plans/                       epics/stories (See plans/AGENTS.md)
├── specs/                       backend/UI/chat specs
├── reference/                   ACP/MCP references
├── reviews/<date>/              security/audit findings
└── archive/, development/, research/, rust-ecosystem/  deep dives
```

## Rules & Patterns

- Keep plans concise and executable in one branch.
- Update plan status by renaming files or editing `STATUS.md`.
- Save security audit findings in `docs/reviews/<date>/`.
