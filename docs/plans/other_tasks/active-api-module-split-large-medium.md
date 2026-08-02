# API module split

> Difficulty: large. Urgency: medium. Status: active.

## Goal

Reduce `src/api/mod.rs` from a mixed router, handler, middleware, helper, and test
megafile into focused Rust modules without changing HTTP behavior or security
boundaries.

## Scope

- Keep router composition, `AppState`, shared API helpers, and error plumbing in
  `src/api/mod.rs`.
- Extract cohesive route groups into private sibling modules, following the
  existing `mcp`, `profiles`, `providers`, `session_extra`, and `settings`
  patterns.
- Co-locate or otherwise relocate the relevant API tests and preserve shared
  test-state construction.
- Avoid unrelated behavior changes, frontend changes, dependency changes, and
  public API changes.

## Execution phases

1. Primary-b reviews the current module and proposes boundaries, dependency
   visibility, test movement, and safe sequencing.
2. Extract shared test support and the first low-coupling route groups; run the
   Rust formatting, lint, and test gates.
3. Extract the remaining route groups and tests; run the same gates again.
4. Review the final diff for behavior, visibility, route registration, and
   security regressions; run the unified project gate.

## Acceptance criteria

- `src/api/mod.rs` is a composition-oriented module rather than a handler
  megafile; each extracted module has one cohesive responsibility.
- All existing Rust tests pass, including API and contract-facing tests.
- `cargo fmt --check`, Clippy with warnings denied, and the repository's
  required checks pass after every implementation phase.
- Route paths, body limits, middleware ordering, authorization, path validation,
  and error response shapes remain unchanged.
- No unrelated existing worktree changes are modified.

## Verification

Per phase:

```text
cargo fmt -q --check
cargo clippy -q --all-targets -- -D warnings
cargo test -q --all-targets
```

Final gate: `make check`.

## Handoff

Suggested commit: `refactor(api): split route handlers into focused modules`
