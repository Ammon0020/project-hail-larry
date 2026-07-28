---
name: comprehensive-review
description: Parallel subagent code review — splits the surface area into groups, dispatches 4 small subagents per batch, each writes one markdown file per action item to docs/reviews/<date>/
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

Review a code surface area by splitting it across parallel `small` subagents. Each subagent walks its files one at a time and writes one markdown file per action item.

## Inputs

- **Base ref** (optional): ref to diff against. Defaults to `HEAD` (all uncommitted: staged + unstaged + untracked).

## Workflow

### 1. Decide the surface area

```sh
git status --short
git diff --stat          # unstaged
git diff --cached --stat # staged
git diff --stat <base>   # if a base ref is given
git status --porcelain | grep '^??'  # untracked
```

If nothing changed, say so and stop. Note which files are staged vs unstaged vs untracked — subagents need to know which `git diff` variant to run.

### 2. Create a todo list to iterate through

Use `todo_write` with one item per batch of 4 subagent reviews, plus an item for the final summary. Group changed files into ~4 logical review units per batch (by package, feature area, or file type).

### 3. Dispatch 4 subagents per batch

Dispatch **4 `small` subagents in parallel** per batch (all 4 `run_subagent` calls in one message, `is_background: true`).

**Required subagent instructions** (include in every prompt):
- `IMPORTANT: Do NOT run build/test commands. Only use read, grep, and git diff. Keep all commands short and synchronous.`
- State the exact files to review and which `git diff` variant to run.
- Tell subagents to return "no findings" explicitly if clean.

**Per-subagent process** (spell this out in the prompt):
1. Create a todo list of all items to explore in the assigned files.
2. Read **one** file. Describe its architecture. Call out:
   - Bugs (race conditions, leaks, error-handling gaps, security, cross-platform)
   - Optimization opportunities
   - Overly complex or verbose code (suggest refactors)
3. Create **one markdown review file per action item** under `docs/reviews/<YYYY-MM-DD>/` with 1–3 fix options. Skip fixes entirely if the item needs a decision or senior dev — just describe it.
4. Read the next file. Describe how it interacts with the previous file(s) if it does. Repeat until all assigned files are reviewed.

**Combining items**: if two action items can be fixed in one change, put them in the same file.

**Finding file format**:
```
- name: short descriptive title
- difficulty: [trivial, easy, medium, hard]   (how hard to fix)
- urgency: [low, medium, high, critical]      (how important before commit)
- file: absolute path
- lines: line range (e.g. "45-60")
- description: what the issue is and why it matters
- options: 1–3 fix options (or skip if it needs a decision)
- verification: how you confirmed this is real (e.g. "read line X, the goroutine at Y has no cancellation path")
```

### 4. Collect, then dispatch the next batch

After a batch, `read_subagent` with `block: true` for all 4. Collect findings, then dispatch the next 4. Kill any subagent stuck looping on background commands and re-dispatch with stronger instructions.

### 5. Stop if findings stack up too high

If findings pile up, stop and note where you are (which batches are done, which remain). Suggested refactors can fix many findings at once — we may want to fix findings then pick up again.

### 6. Write the README index

Create `docs/reviews/<YYYY-MM-DD>/README.md` with:
- Scope reviewed, subagent/batch count
- Total findings and breakdown by urgency
- Tables grouping findings by urgency with file links and difficulty
- Notes on duplicate root causes or pre-existing issues

### 7. Final summary to the user

- Total findings, breakdown by urgency
- The 6 highest-urgency findings called out by name
- Reference to the README index

## File naming

Finding files: `<slug>,<difficulty>,<urgency>.md` (kebab-case slug derived from the title).
Example: `uploads-sessionid-path-traversal,medium,high.md`

Status suffixes (managed by `resolve-reviews`, not this skill): `,in-progress`, `,wontfix`, `,deferred`.

## Pacing rules

- **4 subagents max concurrent** — wait for a batch to finish before dispatching the next.
- **Write files incrementally** — subagents write their own finding files as they go.
- **Mark todos complete immediately** as each batch finishes.
