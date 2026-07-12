---
name: resolve-reviews
description: Triage and resolve a folder of review finding files — validate each is a real and worthwhile issue, plan, implement, verify, then delete or mark wontfix/deferred. Dispatches parallel non-overlapping subagents.
argument-hint: "[reviews-folder]  (default: most recent docs/reviews/<date>/)"
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

# Resolve Review Backlog

Take a folder of review finding files (as produced by `/comprehensive-review`) and work through each one: **validate → plan → implement → verify → delete** (or mark `wontfix`/`deferred`). The directory listing *is* the todo list — pending files are work, resolved files disappear or carry a status suffix.

Validation is the crux: many findings are stylistic or false positives, and some "improvements" (e.g. modularizing a cohesive file) add more complexity than they remove. **`wontfix` is a successful outcome.** Do not implement a fix unless it is a real issue and the fix is worth the churn.

## Inputs

- **Reviews folder** (optional): a folder of `<slug>,<difficulty>,<urgency>[,status].md` files. Defaults to the most recent `docs/reviews/<YYYY-MM-DD>/` — list `docs/reviews/`, sort date folders descending, pick the first that contains pending finding files (skip folders where every `.md` is `README.md` or already carries a status suffix).

## File naming convention

Review files are `<slug>,<difficulty>,<urgency>.md` and may carry a trailing status segment:

| Suffix | Meaning |
|---|---|
| `<slug>,<diff>,<urg>.md` | Pending — not yet worked |
| `...,in-progress.md` | Actively being worked (complex items only) |
| `...,done.md` | Fixed and verified (transient — deleted at end) |
| `...,wontfix.md` | Validated as not a real issue / not worth fixing; reasoning appended |
| `...,deferred.md` | Real but deferred; recorded in `docs/known-issues.md` |

Ignore `README.md` and any non-`.md` files. Treat `in-progress`/`done`/`wontfix`/`deferred` files as already-resolved (verify `done` files get deleted; never re-queue the others).

## Workflow

### 1. Build the backlog

List the reviews folder. For each finding `.md` (except `README.md`), parse the filename into `slug`/`difficulty`/`urgency`/`status`, then read the body to extract the `File:` and `Lines:` fields plus any other file paths referenced in the description/recommendation. Build a table:

| slug | difficulty | urgency | status | primary files | all files touched | title |

Sort the pending queue:
1. Urgency: `critical` → `high` → `medium` → `low`
2. Within urgency, difficulty: `trivial` → `easy` → `medium` → `hard`

Use `todo_write` with one item per pending finding (if >20 items, group into batch todos of ~5).

If there are no pending findings, say so and stop.

### 2. Classify into execution tiers

For each pending item, decide how it runs based on its file footprint and difficulty:

- **Parallel-safe:** touches 1–2 specific files, `trivial`/`easy`/`medium` difficulty, no broad refactor. Can run in parallel with other parallel-safe items **as long as no two concurrent subagents touch the same file**.
- **Serial:** `hard` difficulty, OR touches >2 files, OR is a cross-cutting refactor (god-struct decomposition, interface changes, cross-package deduplication, dependency additions). Run these **one at a time**.

### 3. Group parallel-safe items into file-disjoint batches

Build a conflict graph: two items **conflict** if their `all files touched` sets intersect. Greedily pack items into dispatch rounds such that **across all subagents running concurrently, no file is touched by more than one subagent**.

Practical method:
- Group items by primary file — all items touching the same file go to the same subagent.
- Cluster non-conflicting single-file items onto one subagent (cap ~3 items per subagent so reports stay readable).
- Dispatch one round = up to 4 subagents whose file sets are pairwise disjoint.
- Any item that conflicts with everything in the current round waits for the next round (or gets merged with the subagent that owns the shared file).

### 4. Dispatch subagents

**Subagent tier by difficulty:**
- `trivial` → `trivial` profile
- `easy` → `small` profile
- `medium` → `routine` (or `small`)
- `hard` / serial refactors → `primary-a`

Dispatch parallel-safe rounds with `is_background: true`, up to **4 concurrent**. Wait for the whole round (`read_subagent` with `block: true` on each) before dispatching the next round. Re-check file-disjointness against any items whose scope widened (see `NEEDS-SCOPE` below).

**Subagent task prompt** (adapt per item; include this verbatim guidance):

> You are resolving one or more code review findings. Do NOT run long-running or background commands; keep all commands short and synchronous.
>
> Review file(s): <paths>
> Files you may edit: <explicit list>
>
> For EACH finding, perform these steps in order:
>
> 1. **Read the review file** and **read the actual code at the cited lines**. Confirm the issue still exists — the code may have changed since the review. If the cited lines no longer match, locate the current equivalent before judging.
>
> 2. **VALIDATE** — decide whether this is a real issue AND worth fixing. Apply real judgment, do not rubber-stamp:
>    - **Real bug / security / correctness / data-loss / race** → almost always worth fixing.
>    - **Maintainability / style suggestion** → weigh benefit vs. churn and risk. A forced modularization of a cohesive file can make code *harder* to follow; a 14-line hand-rolled helper may not justify a new dependency. **Don't fix if the cure is worse than the disease.**
>    - **Blast radius** — will the fix risk breaking working code? Is that acceptable for the benefit? A risky refactor of working code for a stylistic gain is not worth it.
>    - **Duplicate / already-addressed** — if the issue was already fixed or duplicates another finding, it's not worth fixing.
>    - **False positive** — if the cited code doesn't actually have the problem, it's not a real issue.
>
> 3. **If NOT worth fixing:** STOP, do not edit code. Report exactly: `WONTFIX — <slug> — <specific reason>` (e.g. *"the custom magic-byte check is 14 lines, tested, and avoids a dependency; replacing it adds churn for no real gain"*).
>
> 4. **If worth fixing but large/risky and not urgent:** STOP, do not edit code. Report exactly: `DEFER — <slug> — <reason> — suggested path: <concrete fix approach>`.
>
> 5. **If worth fixing:** PLAN the minimal fix, IMPLEMENT it, then VERIFY with the *targeted* commands relevant to the change:
>    - Go: `go test ./<affected-pkg>/...` (package-scoped, not `./...`), `go vet ./<affected-pkg>/...`
>    - Frontend: `npm run build` only if frontend files changed
>    - Lint: `golangci-lint run` for changed Go packages (quiet flags where possible)
>    - Only run what is relevant to the change. Do not run the full suite unless the change is cross-package.
>    - If verification fails, fix and retry — do not report success until it passes.
>
> 6. Report exactly: `DONE — <slug> — <one-line change summary> — verified via <commands>`.
>
> **Hard constraints:**
> - Only edit files in the list above. If the fix genuinely requires a file outside that set, STOP and report `NEEDS-SCOPE — <slug> — <file> — <why>`.
> - Do not run `go test ./...`, full builds, or any background/long-running command.
> - Do not add new dependencies without reporting `NEEDS-SCOPE — <slug> — dependency <name> — <why>` first.
> - One report line per finding, prefixed with `DONE` / `WONTFIX` / `DEFER` / `NEEDS-SCOPE`.

### 5. Handle subagent results

For each subagent report, act per finding:

- **DONE** → spot-check the edited file(s) yourself (read the changed region) and run the targeted test/lint once. If good: **delete the review file**. If it was renamed to `in-progress`, delete it. Update `docs/STATUS.md` if the change is user-visible or closes a tracked gap.
- **WONTFIX** → append a `## Resolution` section to the review file:
  ```markdown
  ## Resolution (<YYYY-MM-DD>) — WONTFIX
  <reason from subagent>
  ```
  Then rename the file to `...,wontfix.md`. **Do not delete** — the wontfix record prevents the next review from re-raising it.
- **DEFER** → add a one-line entry to `docs/known-issues.md` under a `## <YYYY-MM-DD> review — deferred` section, with severity (from urgency) and a concrete fix path. Then rename the review file to `...,deferred.md`.
- **NEEDS-SCOPE** → the fix needs files/deps outside its assigned set. Re-plan: widen the scope and re-run it as a **serial** item (so it can't conflict with parallel work), or merge it with the subagent that owns the other file. Rename to `...,in-progress.md` while re-planning.
- **Subagent stuck/looping** on builds or tests → `kill_shell` it and re-dispatch with stronger constraints (reiterate: no full builds, no background commands).

### 6. Serial items

For each serial (`hard` / cross-cutting) item, run **one at a time**:

1. Rename the review file to `...,in-progress.md` (so the directory reflects active work).
2. Dispatch a single `primary-a` subagent with the same task template, but give it the **full explicit list of files it may touch** and allow package-scoped tests across the affected packages. Tell it the scope is broad and to report `NEEDS-SCOPE` only if it must touch files outside that full list.
3. On **DONE** → verify (spot-check + targeted tests), then **delete** the review file. On **WONTFIX**/`DEFER` → as in step 5 (revert the `in-progress` suffix to `wontfix`/`deferred`). On `NEEDS-SCOPE` → widen and retry, or escalate to the user if truly unbounded.

Do not run a serial item in parallel with any other item that touches overlapping files.

### 7. Verify the whole batch

After all items are resolved, run project-wide verification **once**:
- `go vet ./...`
- `go test ./...` (quiet flags)
- `npm run build` if any frontend files changed
- `golangci-lint run` if any Go files changed

If anything fails, fix it before finishing (the failure almost certainly comes from one of this session's changes). If a failure is pre-existing and unrelated, note it in `docs/known-issues.md` and move on.

### 8. Final summary

Output to the user:
- Counts: `done` / `wontfix` / `deferred` / `still-pending`
- `wontfix` items with one-line reasons (so the user can override if they disagree)
- `deferred` items with links to their `docs/known-issues.md` entries
- Any `still-pending` items with the blocker (e.g. `NEEDS-SCOPE` awaiting a decision)
- Reference to the reviews folder path

## Pacing & safety rules

- **Max 4 concurrent subagents.**
- **File-disjointness is mandatory across all concurrent subagents** — never let two running subagents edit the same file. This is what makes parallelism safe.
- **Serial items run alone** — don't dispatch a serial item alongside anything touching overlapping files.
- **Validate before implementing** — the validate step is not optional and not a formality. Be honest: a stylistic finding that doesn't clearly improve the code should be `wontfix`, not a forced change.
- **Delete on done; record on wontfix/defer** — keeps the directory a clean todo list while preserving `wontfix`/`deferred` reasoning so findings aren't re-raised.
- **Don't add dependencies or widen scope without surfacing it** (`NEEDS-SCOPE`) — the orchestrator decides whether to widen or escalate.
- **Mark todos complete immediately** as each finding resolves.
