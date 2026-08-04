# API module split

> Difficulty: large. Urgency: medium. Status: complete.

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

## Recommended design

Use feature-oriented sibling modules with one central route table. Keep
`router()`, `AppState`, global response hardening, shared pending-action helpers,
and API error plumbing in `mod.rs`. Extract these groups:

- `auth.rs` — authenticated request edge, peer extraction, and WebSocket auth
  checker.
- `pair.rs` — pairing endpoints, device-name validation, and pair rate limiting.
- `devices.rs` — device listing/revocation and pending actions.
- `workspaces.rs` — workspace registration, removal, and trust operations.
- `files.rs` — file tree/read/write/mutation/search and shared preview serving.
- `preview.rs` — preview tickets, cookie authorization, and preview serving.
- `events.rs` — event query endpoints and limit clamping.
- `agents.rs` — agent CRUD and cached autodetection.
- `sessions.rs` — session lifecycle, prompts, profiles, and session validation.
- `permissions.rs` — permission listing and responses.
- `test_support.rs` — shared `cfg(test)` state and request harness helpers.

Move `spa_fallback` into the existing `embed.rs` and `ServerSettings` into
`settings.rs`, where their only consumers already live. Leave `git.rs` and the
other existing feature modules untouched. Preserve the single route inventory
and middleware/body-limit ordering in `router()`.

## Execution phases

1. Primary-b verifies the current structure, compares alternative boundaries,
   and produces the dependency, visibility, test, and sequencing plan.
2. Move `test_support` and the low-coupling edge modules (`auth`, `agents`,
   `events`, `permissions`), plus `spa_fallback` and `ServerSettings`. Run
   formatting, Clippy, and all Rust tests.
3. Move `pair` and `devices`, then run the same gates.
4. Move `sessions`, then run the same gates.
5. Move `workspaces`, `files`, and `preview`, co-locating their tests. Run the
   same gates and compare route/test inventories.
6. Update API module documentation, inspect the security-sensitive diff, and
   run `make check`.

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

## Result

Completed with feature-oriented modules for auth, pairing, devices, workspaces,
files, previews, events, agents, sessions, and permissions. Shared test support
and API tests were relocated without duplicate coverage. `mod.rs` is now a
composition-oriented 701-line module, while the route groups own their handlers.

Verification completed after each extraction phase and at the final gate:

- Rust format check, Clippy with warnings denied, and all 511 Rust tests passed.
- Frontend ESLint and production build passed.
- Contract suite passed: 82 tests.

## Handoff

Suggested commit: `refactor(api): split route handlers into focused modules`
