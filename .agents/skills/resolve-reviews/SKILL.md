---
name: resolve-reviews
description: Subagent-driven review fixer — processes action item finding files in batches, delegates fixes to subagents, and validates via parent-level tests, linting, and formatting before suggesting commits.
argument-hint: "[dir] (default: docs/reviews/<YYYY-MM-DD> or latest review dir)"
allowed-tools:
  - read
  - grep
  - glob
  - exec
  - write
  - edit
  - run_subagent
  - read_subagent
  - todo_write
---

# Fix Reviews

Batch-fix review action items using parallel `small` subagents, validate changes centrally at the parent level, and recommend git commit messages per resolved batch.

## Workflow

### 1. Collect & Group Action Items
1. Locate finding files in `docs/reviews/<YYYY-MM-DD>/` (or passed path).
2. Read files and group related findings into logical batches (max 3 concurrent fixes per batch). Focus on tightly coupled files or individual `<slug>,<difficulty>,<urgency>.md` units meant to be fixed together.

### 2. Dispatch Fix Subagents (Max 3 Concurrent)
Dispatch up to **3 `small` subagents in parallel** (`is_background: true`).

**Instructions to include in every subagent prompt:**
1. Read the assigned finding file(s) and target code files.
2. Apply the required code modifications using `edit` or `write`.
3. Do NOT run tests, linters, or git operations—leave validation to the parent.
4. Report completed changes and any unexpected edge cases encountered.

### 3. Validate Changes (Parent Level)
Once a batch finishes, run project checks at the parent level:
```sh
# Run relevant formatters, linters, and test suites
npm run format / cargo fmt / ruff format
npm run lint / cargo clippy / ruff check
npm test / cargo test / pytest
```
- **If checks fail**: Dispatch a targeted subagent or fix directly using check errors.
- **If checks pass**: Mark the corresponding finding files with the `,done` suffix (or move/delete according to your workflow).

### 4. Recommend Commit Message
After each successfully validated batch, provide a clean, descriptive commit message recommendation:

> **Suggested Commit:**
> `fix(scope): brief summary of fixed action items`
>
> - Resolved `<slug-1>`
> - Resolved `<slug-2>`

### 5. Repeat
Proceed to the next batch until all targeted finding files are resolved.