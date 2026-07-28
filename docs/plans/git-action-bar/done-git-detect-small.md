# S-GIT-DETECT — Workspace git detection

> Story. Difficulty: small. Urgency: medium. Epic: `pending-git-action-bar-large.md`.

## Goal

Let the daemon answer "is this workspace a git repo, and if so what branch?"
without shelling out, and surface that to the frontend so the hardcoded
`"main"` in `EditorPane.tsx:496` is removed.

## Scope

- Add `src/git/mod.rs` (new crate module) with a `detect(root: &Path)`
  function backed by `gix`. Returns `Option<GitRepoInfo>` where
  `GitRepoInfo { head_branch: Option<String>, head_oid: Option<String>,
  is_shallow: bool, has_uncommitted_changes: bool }`.
- Open the repo read-only (`gix::open`). Reject symlinks inside `.git/`
  consistent with the existing `workspace/` symlink policy.
- Add `GET /api/workspaces/{id}/git` returning the `GitRepoInfo` plus
  `repo_detected: bool`. Authenticated (same gate as other workspace reads).
  When no repo: `200` with `repo_detected: false` and null fields — never 404.
- Frontend: fetch via `useBackend` (or new `useGitState` hook), replace the
  hardcoded `"main"` in `EditorPane.tsx:496` BreadcrumbBar call with the live
  branch when a repo is detected; fall back to workspace name when not.
- Cache the detection per workspace in daemon state; invalidate on workspace
  file-watcher events under `.git/HEAD` or `.git/refs/`.

## Out of scope

- Status/diff/stage/commit/push (S-GIT-API).
- Action bar UI (S-GIT-ACTION-BAR) — this story only feeds the breadcrumb.

## Library

`gix = "0.x"` (latest stable at impl time, ≥7 days old per repo policy). Pure
Rust, no libgit2. Add to `Cargo.toml` under a new `# --- Git ---` section with
the same justification style as existing entries.

## Acceptance

- [ ] `GET /api/workspaces/{id}/git` returns `repo_detected: true` + branch
      for a real repo, `repo_detected: false` for a plain folder.
- [ ] `EditorPane.tsx` no longer hardcodes `"main"`; breadcrumb shows the
      live branch on a repo, workspace name otherwise.
- [ ] Symlinked `.git` is rejected and logged.
- [ ] Cache invalidates on `HEAD`/`refs/` writes (covered by a unit test).
- [ ] `cargo test -q --all-targets`, `cargo clippy -q --all-targets -- -D
      warnings`, `cargo fmt -q --check`, frontend `npm run lint --silent` +
      `npm run build --silent`, and `make test-contract` all pass.

## Verification

- Rust unit test against a temp dir with `git init` (use the `git` CLI in the
  test fixture to set up state, not in production code).
- Contract test for the new endpoint in `tests/contract/`.

Suggested commit: `feat(git): workspace repo detection (S-GIT-DETECT)`
