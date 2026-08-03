# Git module split

> Difficulty: large. Urgency: high. Status: complete.

## Goal

Reduce `src/git/mod.rs` without changing the Git API, path-security behavior,
or CLI credential handling. This is the first target in the repository
modularization queue because Git work is actively growing.

## Scope

Use a thin facade with focused siblings:

```text
src/git/
├── mod.rs       public facade/re-exports
├── types.rs     DTOs and GitError
├── repo.rs      detection/status/path containment
├── history.rs   log and graph labels
├── worktree.rs  diff/stage/commit/push/init/gitignore
├── cli.rs       git subprocess helpers
└── tests.rs     existing inline Git tests
```

Move code mechanically. Preserve `pub` signatures, serialized DTO shapes,
symlink rejection, workspace path containment, diff-size caps, and the special
`git push` environment behavior. Do not split tiny concerns into standalone
files or change the Git implementation while moving it.

## Acceptance

- `src/git/mod.rs` is a small public facade with unchanged exports.
- Production responsibilities are separated by the boundaries above.
- Tests move to `src/git/tests.rs` without coverage loss or golden changes.
- API callers require no behavior or import changes.
- No unrelated frontend, API, or configuration changes are included.

## Verification

Run after each extraction phase and before handoff:

```text
cargo fmt --all -- --check
cargo clippy -q --all-targets -- -D warnings
cargo test -q --all-targets
make check
```

Compare Git route/DTO contract tests and inspect the final diff for path,
symlink, subprocess, credential, and output-size regressions.

## Result

Completed as a mechanical split into `types.rs`, `repo.rs`, `history.rs`,
`worktree.rs`, `cli.rs`, and `tests.rs`. `src/git/mod.rs` is now a 34-line
facade with unchanged `crate::git::*` exports. The four Git contract goldens
remain unchanged.

Verification completed after every extraction phase and at the final gate:

- Rust format check, Clippy, and all 511 Rust tests passed.
- Frontend lint and production build passed.
- Contract suite passed: 82 tests.
- `cargo doc` was retried quietly with one job; it remains blocked by
  pre-existing rustdoc private/broken-link warnings outside `src/git`.

## Handoff

Suggested commit: `refactor(git): split repository operations into modules`
