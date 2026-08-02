# Git module split

> Difficulty: large. Urgency: high. Status: active.

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

## Handoff

Suggested commit: `refactor(git): split repository operations into modules`
