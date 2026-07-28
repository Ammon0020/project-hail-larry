---
name: resolve-reviews
description: Triage and resolve a folder of review finding files — validate each is a real and worthwhile issue, plan, implement, verify, then delete or mark wontfix/deferred. Dispatches parallel non-overlapping subagents.
argument-hint: "[reviews-folder]  (default: most recent docs/reviews/<date>/)"
---

# Resolve Review Backlog

Read review finding files (e.g. `docs/reviews/<date>/`), check if they're valid, and fix them.

## File Naming & Status
Files are named `<slug>,<difficulty>,<urgency>.md`. They may have a status suffix:
- **Pending**: `<slug>,<diff>,<urg>.md` (Work on these)
- **in-progress**: `...,in-progress.md`
- **wontfix**: `...,wontfix.md` (Valid outcome if not worth fixing. Append `## Resolution` and reason, rename, do NOT delete).
- **deferred**: `...,deferred.md` (Log in `docs/known-issues.md`, rename, do NOT delete).
- **done**: Delete these when fixed and verified.

## Execution Rules
1. **Parallel execution**: Run up to 4 subagents concurrently for simple fixes. 
2. **Strict File-Disjointness**: Never let two concurrent subagents edit the same file.
3. **Serial execution**: Cross-cutting or `hard` difficulty items MUST run one at a time.
4. **Verification**: After all items are resolved, run project-wide verification (`make check` for the full suite, or `make lint` for a fast style/correctness pass).
