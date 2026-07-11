---
name: comprehensive-review
description: Parallel subagent code review — splits changes into groups, dispatches 4 small subagents per batch, verifies findings, and writes each finding to its own markdown file under docs/reviews/<date>/
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

# Pre-commit Parallel Review

Review all uncommitted changes by splitting the work across parallel `small` and `trivial` subagents, verifying each finding, and writing each finding to its own actionable markdown file.

## Inputs

- **Base ref** (optional): The ref to diff against. Defaults to `HEAD` (all uncommitted: staged + unstaged + untracked). If the user provides a branch/tag/sha, diff against that instead.

## Workflow

### 1. Scope the changes

```sh
git status --short
git diff --stat          # unstaged
git diff --cached --stat # staged
git diff --stat <base>   # combined if a base ref is given
```

Identify untracked files and directories too (`git status --porcelain | grep '^??'`). Note which files are staged vs unstaged vs untracked — subagents need to know which `git diff` variant to run.

If there are no uncommitted changes, say so and stop.

### 2. Create a plan and todo list

Use `todo_write` to create a todo list with:
- One item per batch of 4 subagent reviews
- An item for writing finding files
- An item for the final summary

Group the changed files into logical review units (by package, feature area, or file type). Aim for 4 units per batch. Examples of groupings:
- Backend packages (fswatch, uploads, acp, events, server, daemon, pairing, config, workspace, interfaces)
- Frontend areas (App+theme, shadcn/ui, chat components, other components, hooks+api+types)
- Docs and config (docs, .devin/agents, specs, go.mod/package.json/eslint)

### 3. Dispatch subagents in batches of 4

For each batch, dispatch **4 `small` subagents in parallel** (all 4 `run_subagent` calls in a single message, `is_background: true`).

**Critical subagent instructions** (include in every task prompt):
- `IMPORTANT: Do NOT run any long-running or background commands. Do NOT run go test, npm run build, go build, or any build/test commands. Only use read, grep, and git diff. Keep all commands short and synchronous.`
  - This prevents subagents from getting stuck in loops running builds or tests.
- State the exact files to review and which `git diff` variant to run (staged/unstaged/untracked).
- List the specific issue categories to look for (see below).
- Require a structured finding format (see below).
- Tell subagents to report ONLY real, verified findings with precise line numbers.
- Tell subagents to return "no findings" explicitly if clean.

**Issue categories to instruct subagents to check** (tailor per area):

Backend (Go):
- Bugs, race conditions, goroutine/resource leaks, error handling gaps
- Security: path traversal, injection, missing auth, info disclosure, DoS
- Cross-platform issues (Windows/macOS/Linux)
- Concurrency: data races, deadlocks, missing locks
- Test correctness: do tests actually test what they claim?
- Interface/API design: breaking changes, missing methods, leaky abstractions
- Deviations from AGENTS.md conventions

Frontend (React/TypeScript):
- React bugs: stale closures, missing effect deps, missing cleanup, state management
- Accessibility: missing aria attributes, focus management, keyboard navigation
- TypeScript type safety gaps
- Memory leaks: unrevoked object URLs, uncleaned event listeners
- Tailwind CSS standards: semantic tokens not raw colors, no inline CSS, @apply misuse
- Deviations from AGENTS.md Tailwind CSS Standards

Docs/Config:
- Accuracy: do docs match the actual implementation? Verify claims by grepping code
- Broken cross-references: do referenced files/paths exist?
- Stale information, missing documentation for new features
- Dependency issues: unused deps, version pinning, security concerns
- Build config changes that may hide real problems

**Required finding format** (tell each subagent to report this):

```
- name: short descriptive title
- difficulty: one of [trivial, easy, medium, hard]  (how hard to fix)
- urgency: one of [low, medium, high, critical]     (how important to fix before commit)
- file: absolute path
- lines: line range (e.g. "45-60")
- description: what the issue is and why it matters
- recommendation: how to fix
- verification: how you confirmed this is a real issue (e.g. "read line X, the goroutine at Y has no cancellation path")
```

### 4. Collect results

After dispatching a batch of 4, wait for all 4 to complete (`read_subagent` with `block: true`). Collect all findings before starting the next batch.

If a subagent gets stuck looping on background commands, kill it with `kill_shell` and re-dispatch with stronger instructions to avoid build/test commands.

### 5. Verify findings

Before writing a finding file, spot-verify a few high-urgency findings by reading the actual code at the cited lines. This catches false positives from subagents. You don't need to re-verify every low-urgency finding — focus on high and medium urgency.

### 6. Write finding files

Create a directory: `docs/reviews/<YYYY-MM-DD>/`

Write **each finding as its own markdown file**. Do not batch multiple findings per file.

**File naming**: `<slug>,<difficulty>,<urgency>.md`
- Slug: kebab-case short name derived from the finding title
- Example: `uploads-sessionid-path-traversal,-medium,-high.md`

**File content template**:

```markdown
# <Finding title>

- **Difficulty:** <trivial|easy|medium|hard>
- **Urgency:** <low|medium|high|critical>
- **File:** `<path>`
- **Lines:** <range>

## Description

<description from subagent>

## Recommendation

<recommendation from subagent>

## Verification

<verification from subagent>
```

Write files in small batches of 3-4 `write` calls per message to avoid overloading the renderer. Do not write all files in a single mega-script.

### 7. Write the README index

Create `docs/reviews/<YYYY-MM-DD>/README.md` with:
- Summary of scope (what was reviewed, how many subagents/batches)
- Total finding count and breakdown by urgency
- Tables grouping findings by urgency with file links and difficulty
- Notes on duplicate root-cause findings or pre-existing issues

### 8. Final summary

Output a summary to the user with:
- Total findings count
- Breakdown by urgency (high/medium/low)
- The 6 highest-urgency findings called out by name
- Reference to the README index file

## Pacing rules

- **4 subagents max concurrent** — wait for a batch to finish before dispatching the next.
- **Write files incrementally** — write finding files as batches complete, not all at the end. 3-4 `write` calls per message.
- **Don't stop until done** — keep dispatching batches and writing files until all changed files are reviewed and all findings are written.
- **Mark todos complete immediately** as each batch finishes and as files are written.
