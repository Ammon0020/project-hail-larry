---
name: compact-code
description: Compact a file or area — reduce tokens by deduplicating, simplifying, and collapsing real duplication, without stripping comments, hurting readability, or trading line count for opacity.
argument-hint: "[file or path]"
allowed-tools:
  - read
  - grep
  - glob
  - edit
  - write
  - exec
  - find_file_by_name
  - todo_write
  - run_subagent
  - read_subagent
---

# Compact Code

Reduce the token cost of a target file or area **without losing overall app functionality or safety**. Real wins come from removing genuine duplication, simplifying over-engineered logic, and collapsing abstractions that don't pay for themselves. This is not a formatter, not a minifier, and not a license to gut code.

Basically, where less code is more maintainable, use less code. For large-scale refactors, suggest a fix in a markdown file for later review. 

## Core principle

A successful compaction **reduces the number of tokens a reader (human or model) must process to understand and safely modify the code**, while preserving behavior and safety. Line count is a proxy, not the goal. A change that drops 200 lines but hides a bug, removes a guard, or makes the code harder to follow is a failure.

## What counts as a win

- **Real duplication** — two or more blocks doing the same work with cosmetic differences. Extract a helper, a method, or a small shared module *only if the shared thing is used at least twice after extraction* and the call sites stay readable.
- **Parallel conditionals / switch ladders** that map inputs to outputs in the same shape — collapse into a lookup table, registry, or single dispatch when the result is shorter *and* clearer.
- **Dead or unreachable code** — flags that are never read, branches that can never be taken, helpers with no callers. Remove after confirming with grep across the whole repo (not just the file).
- **Over-abstraction** — an interface with one implementation, a factory that returns one thing, a wrapper that adds a layer but no behavior. Inline it. This is the most common "stealth bloat" and the most valuable to remove.
- **Redundant indirection** — `getX()` that just returns `x`, middleware that passes through unchanged, configs that re-state defaults. Collapse.
- **Unusable or duplicated features** — if a feature is genuinely unreachable from the UI, duplicated by another path, or stubbed in a way that misleads, **reducing app functionality is acceptable**. But you must verify unreachability (grep callers, trace routes, check the UI) and call it out explicitly in your summary. When in doubt, keep the feature and flag it for the user instead of deleting.
- **Verbose patterns** that have a shorter idiomatic equivalent in the language (early returns, guard clauses, language builtins, stdlib functions the author reinvented).

## What is NOT a win — do not do these

- **Stripping or thinning comments.** Comments and docstrings are load-bearing context. Keep them. You may *fix* a comment that is now wrong because of your edit, or tighten one that is bloated, but never remove commentary to hit a token target. If a comment explains *why*, it stays.
- **One-lining multi-line constructs** — collapsing a readable block onto one line, putting CSS rules on one line, chaining calls into a single giant expression. This reduces line count but not tokens (often increases them via punctuation), and destroys readability and diff quality. **Never do this.**
- **Renaming for brevity** that hurts clarity (`processRequest` → `p`). Shorter names are only a win when the long name was redundant (`userDataObject` → `user`).
- **Inlining a helper that is used twice** to "save the function header." The header costs ~5 tokens once; the duplicated body costs far more and is a future bug source.
- **Splitting one coherent file into N tiny modules** to "be modular." Each new file adds imports, exports, boilerplate, and navigation cost. A module split is a win only when the resulting parts are independently coherent and the total token count (including new imports/exports) goes *down*. Watch for the "modular web" trap — many small files that each depend on the others and add up to more than the original.
- **Removing error handling, validation, guards, or security checks.** Safety is never a compaction target. A `if err != nil { return err }` is not bloat.
- **Removing tests or test assertions.** Tests are not in scope for token reduction. If a test is genuinely redundant (tests the same branch twice), note it for the user; do not delete without confirmation.
- **Golfing** — using obscure language tricks, comma operators, ternary chains, or bit hacks to shorten code. If a reader has to pause to understand it, it failed.
- **Deleting logging.** Logging is debugging infrastructure. You may consolidate duplicate log lines or drop a log that fires on every iteration of a hot loop *only if* the information is preserved elsewhere. When in doubt, keep it.
- **Changing public API or wire formats** to be more compact. That's a refactor with cross-cutting impact, not a compaction. Stay inside the file/module unless the change is clearly internal.

## Workflow

1. **Scope.** Read the target file (or area) fully. Use `grep`/`glob` to find every caller of every exported symbol before changing or removing anything. A function that looks unused inside the file may be called from three other packages.
2. **Inventory candidates.** Make a mental or written list of: duplicated blocks, single-impl interfaces, dead branches, verbose patterns, unreachable features. Rank by expected token savings and risk.
3. **Plan with `todo_write`.** One todo per concrete change. Group small related changes into one todo. This keeps the work reviewable and prevents drive-by edits.
4. **Edit conservatively.** Make one logical change at a time. After each, re-read the surrounding code to confirm you didn't break a caller, a type, or a guard.
5. **Verify.**
   - Build/lint: run the project's build and linter (`./build.sh`, `go vet ./...`, `npm run build`, `golangci-lint run`, `eslint`) — whichever apply. Quiet flags where possible.
   - Tests: run `go test ./...` (or the project's test command) for the affected packages. **Do not mark the task complete if tests you touched are failing** — keep going until green or until you've identified a pre-existing failure (note it in `docs/known-issues.md` and move on, per AGENTS.md).
   - Behavior spot-check: for any logic you collapsed (e.g. a switch → map), trace one or two concrete inputs through both the old and new form to confirm identical output.
6. **Summarize.** Report, per change: what was removed/collapsed, approximate token savings, any feature reduction (flagged explicitly), and verification run. If you reduced app functionality, say so plainly — do not bury it.

## Subagent use

For a large target (a whole package or several files), dispatch `small` or `routine` subagents in parallel to inventory candidates in different files — **read-only** (`subagent_explore`). Collect their findings, then make the edits yourself in the main context so the changes stay coherent and cross-file dedup is visible in one place. Do not let subagents edit in parallel on overlapping areas; they will produce conflicting refactors.

## Red flags to stop and ask the user

- The only wins you can find require changing a public API, a wire format, or a stored data shape. Stop — that's a refactor, not a compaction.
- A "duplication" is actually two things that look alike but differ in a security-relevant way (one checks permissions, one doesn't). Do not merge them. Flag it.
- You're about to remove a feature and you're not 100% sure it's unreachable. Ask first.
- Token savings look great but readability clearly dropped. You went too far. Revert and aim smaller.

## Anti-shortcut checklist (run mentally before finishing)

- [ ] Did I keep every comment that explains *why*?
- [ ] Did I avoid one-lining anything that was readable across multiple lines?
- [ ] Is every extracted helper used at least twice?
- [ ] Did I avoid creating new tiny modules that add more import/export boilerplate than they save?
- [ ] Did I keep all error handling, validation, guards, and security checks?
- [ ] Did I verify callers with grep before removing/renaming anything?
- [ ] Did the build and the affected tests pass?
- [ ] If I reduced functionality, did I say so explicitly in the summary?
