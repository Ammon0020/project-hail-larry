# docs/

## Responsibility

Project documentation, plans, specs, reference, and review findings.

## Module Map

- `STATUS.md` — Current task status (<100 lines, <90 cols).
- `known-issues.md` — Deferred gaps and unrelated test failures.
- **`plans/`** — Epics, stories, and work item plans. (See `docs/plans/AGENTS.md`)
- `specs/` — Backend, UI, and chat-panel specifications.
- `reference/` — ACP and MCP protocol references.
- `reviews/<date>/` — Security/audit findings.
- `archive/`, `development/`, `research/`, `rust-ecosystem/` — Historical or deep-dive docs.

## Rules & Patterns

- Keep plans concise and executable in one branch.
- Update plan status by renaming files or editing `STATUS.md`.
- Save security audit findings in `docs/reviews/<date>/`.
