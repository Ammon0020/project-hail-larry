# Epic: Git Action Bar, Diff Viewer, and SCM Surface

> **Status:** Done (2026-07-26); follow-up `done-git-panel-nonfunctional-medium-high.md` resolved 2026-07-28 (commit 749c0b1). **Difficulty:** large. **Urgency:** medium.
> **Source:** `top-level-todo.md` (Git section). **Created:** 2026-07-26.
> **Related:** `docs/specs/backend-spec.md`, `docs/specs/ui-spec.md`,
> `src/acp/context.rs` (existing `runGit()` agent context only).

## Goal

Add a discoverable, SCM-aware surface to the IDE so a user can see git status,
review per-file diffs, stage, commit, and push from the web/mobile UI when a
git repository is detected in the active workspace. A clickable editor diff
opens from any changed-file list (action bar, future edited-files popup, or
chat) so diff review is a single reusable component, not per-feature code.

## Why now

`top-level-todo.md` calls this **foundational** for the items below it:

- the **edited-files/subagent popup** reuses the same diff viewer
- **git init** reuses the same git-detection infrastructure
- chat auto-naming and other context features benefit from knowing the branch

The backend currently exposes only `runGit()` for agent prompt context — no
git API endpoints exist in `src/api/`. The frontend hardcodes `"main"` in
`EditorPane.tsx:496` for breadcrumb display. This epic closes both gaps with a
stable, modular shape that future stories plug into.

## Architecture decisions (locked for this epic)

- **Library choice — backend:** use `gix` (pure-Rust git implementation, no
  libgit2 C dependency). Workspace containment and path validation already
  live in `src/workspace/`; git reads flow through the same path-gate. No
  spawning of the `git` CLI from the daemon — keeps the security surface in
  Rust and avoids PATH/auth surprises across harnesses.
- **Library choice — frontend diff:** use `@codemirror/merge` (CodeMirror 6
  family, already the editor stack). Reuses existing language packages,
  themes, and the `uiw/react-codemirror` mount pattern.
- **Single reusable diff component:** one `GitDiffViewer` (story
  S-GIT-DIFF-VIEWER) is the only diff surface. Action bar, chat edited-files
  popup, and any future caller route through it via a `path + base + head`
  contract — no per-caller diff rendering.
- **Path containment:** every git operation validates workspace root
  containment before running; results never expose paths outside the
  workspace root. Symlink rejection matches the existing workspace policy.
- **No credentials storage:** `push` uses the agent's environment git
  credentials (SSH agent, credential helper, `GIT_ASKPASS`). The daemon never
  stores or proxies git credentials. If credentials are missing, surface the
  agent's stderr to the user; do not retry silently.
- **Read-only first, write second:** the diff viewer and status read path
  ship before stage/commit/push. Write operations are a separate story so a
  reviewable, reversible MVP lands first.
- **ACP boundary unchanged:** the agent continues to own its own git context
  via `runGit()`. The daemon's new git API serves the *client UI* only — it
  is not injected into agent prompts.

## Story Index

| ID | Story | Size | Depends on | Acceptance |
|---|---|---:|---|---|
| S-GIT-DETECT | [Workspace git detection](git-action-bar/pending-git-detect-small.md) | small | — | Workspace advertises repo state; hardcoded `"main"` removed from `EditorPane.tsx:496` |
| S-GIT-API | [Backend git API: status / diff / stage / commit / push](git-action-bar/pending-git-api-med.md) | med | DETECT | Authenticated, path-contained REST endpoints with `gix` |
| S-GIT-DIFF-VIEWER | [Reusable editor git diff viewer](git-action-bar/pending-git-diff-viewer-med.md) | med | API | `GitDiffViewer` opens from any caller; `@codemirror/merge` render |
| S-GIT-ACTION-BAR | [Git action bar item + status panel](git-action-bar/pending-git-action-bar-med.md) | med | DETECT, DIFF-VIEWER | Action bar item shows branch + change count; panel lists changed files; click opens diff |
| S-GIT-INIT | [Initialize git in workspace](git-action-bar/pending-git-init-small.md) | small | DETECT | `POST /api/workspaces/{id}/git/init` + UI trigger when no repo detected |

**Recommended sequence:** DETECT → API → DIFF-VIEWER → ACTION-BAR → INIT.
API + DIFF-VIEWER can be parallelized once DETECT lands.

## Boundaries

**In scope:** workspace git detection, read-only status/diff API, reusable
diff viewer component, action bar UI with stage/commit/push, git init.

**Out of scope:** branch switching/creating, merge conflict resolution UI,
stash management, blame view, interactive rebase, git credentials storage,
agent-side git context changes (still `runGit()`), pushing tags. Revisit as
follow-up stories after the MVP.

## Cross-cutting risks

- **Path containment:** every endpoint must reject paths that escape the
  workspace root (matches existing `workspace/` policy). Symlinks inside
  `.git` are rejected per the existing workspace symlink policy.
- **Large diffs:** diff output is bounded per file (configurable cap, default
  e.g. 1 MiB) and the API streams/paginates rather than buffering whole repos.
- **Concurrency:** staging/committing while the editor or agent writes a file
  must not race. Re-use the existing revision tracking to detect mid-flight
  edits and refuse a commit with a clear error if the working tree changed.
- **Permissions:** only authenticated paired devices can hit git endpoints
  (same gate as file-write/shell). Stage/commit/push are *write* operations
  and go through the existing permission sink so any paired device can approve.
- **Headless/no-repo workspaces:** every UI surface must gracefully hide when
  no repo is detected — never throw or render an empty git panel.
- **Submodules:** out of scope for MVP. Surface their existence as "submodule
  present, diff not supported" rather than recursing.

## Verification Bar

- Rust unit tests for detection, path containment, diff bounding, and
  stage/commit/push happy path + the refused-mid-edit case.
- Contract tests for each new endpoint (status/diff/stage/commit/push/init)
  in `tests/contract/`.
- Frontend component test for `GitDiffViewer` rendering an added/modified/
  deleted file and for the action bar panel's empty/loading/repo-detected
  states.
- `make check` passes (fmt + clippy + cargo test + frontend eslint/build +
  contract suite).
- Focused security review for path containment, symlink handling, diff DoS
  bounds, and credential non-storage; save in `docs/reviews/<date>/`.

## Handoff

Suggested first commit: `feat(git): workspace repo detection (S-GIT-DETECT)`
