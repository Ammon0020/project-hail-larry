---
name: comprehensive-review
description: Parallel subagent code review — splits the surface area into groups, dispatches up to 3 subagents per batch, each writes markdown files per action item to docs/reviews/<date>/
argument-hint: "[base-ref]  (default: HEAD — reviews all uncommitted changes)"
allowed-tools:
  - read
  - grep
  - glob
  - exec
  - write
  - edit
  - run_subagent
  - read_subagent
  - kill_shell
  - todo_write
  - find_file_by_name
---

# Comprehensive Review

Review a code surface area using parallel `small` subagents (max 3 concurrent). Subagents analyze assigned files sequentially and write grouped action items to finding files.

## Workflow

### 1. Identify Surface Area & Group Files
Run `git status` and `git diff --stat` (against `<base-ref>` if provided) to map changed files. Split files into logical groups for subagents.

### 2. Dispatch Subagents (Max 3 Concurrent)
Dispatch up to **3 `small` subagents in parallel** (`is_background: true`). 

**Instructions to include in every subagent prompt:**
- Do NOT run build or test commands. Use only reading and diffing tools.
- Follow this exact sequence for assigned files:
  1. Read the file.
  2. Summarize its architecture.
  3. Summarize action items.
  4. Write each action item into `docs/reviews/<YYYY-MM-DD>/<slug>,<difficulty>,<urgency>.md`. Combine action items meant to be fixed in the same go into the same file.
  5. Repeat steps 1–4 for the next file.
  6. Summarize connections between all files reviewed so far.
  7. Summarize new action items derived from cross-file interactions (written using the same file naming format).
  8. Repeat the entire process until all assigned files are completed.

### 3. Monitor & Throttle
- Collect subagent outputs.
- **Stop dispatching new subagents** if too many findings pile up—especially large-scale refactors—so they can be addressed first.
- If findings are manageable, dispatch the next batch (max 3 at a time) until the entire review is complete.

## Finding File Format

**Filename**: `<slug>,<difficulty>,<urgency>.md` (e.g., `refactor-session-auth,medium,high.md`)

```markdown
- name: short descriptive title
- file: absolute path
- lines: line range
- description: issue details and code references
- verification: how this was confirmed
```