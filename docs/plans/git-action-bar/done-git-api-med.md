# S-GIT-API — Backend git API: status / diff / stage / commit / push

> Story. Difficulty: medium. Urgency: medium. Epic: `pending-git-action-bar-large.md`.
> Depends on: S-GIT-DETECT.

## Goal

Provide authenticated, path-contained REST endpoints that the action bar and
diff viewer call. All reads use `gix` directly; write operations are gated by
the existing permission sink.

## Endpoints

All under `/api/workspaces/{id}/git`. All require an authenticated paired
device. All `path` query/body params are validated against workspace
containment before any git op.

| Method + path | Body / query | Returns | Notes |
|---|---|---|---|
| `GET /status` | — | `Vec<FileStatus>` + `head_branch`, `upstream`, `ahead`, `behind` | One entry per changed file with `path`, `old_path?`, `status` (`added`/`modified`/`deleted`/`renamed`/`untracked`/`conflicted`), `staged: bool` |
| `GET /diff` | `?path=<rel>&staged=<bool>` | `{ unified: string, base: string, head: string, truncated: bool }` | Bounded (default 1 MiB); `truncated=true` when capped |
| `POST /stage` | `{ paths: string[] }` or `{ all: true }` | `{ staged: string[] }` | Empty `paths` is 400; `all` stages everything tracked + untracked (matching `git add -A`) |
| `POST /unstage` | `{ paths: string[] }` | `{ unstaged: string[] }` | Empty `paths` is 400 |
| `POST /commit` | `{ message: string, amend?: bool }` | `{ oid: string }` | Refuses with 409 if the working tree changed since `/status` was fetched (revision guard) |
| `POST /push` | `{ remote?: string, set_upstream?: bool }` | `{ ok: bool, stderr: string }` | Spawns `git push` (the only endpoint that shells out — `gix` lacks a transport push without creds plumbing). Streams stderr to the response. Never stores or proxies credentials. |
| `POST /init` | — | `{ oid: string }` | S-GIT-INIT story; documented here for API completeness |

## Scope

- New `src/git/ops.rs` with `status`, `diff`, `stage`, `unstage`, `commit`,
  `push`, `init` functions over an opened `gix` repo. Pure-Rust for
  everything except `push`.
- New `src/api/git.rs` REST handlers; mount under the existing authenticated
  router. **Auth model correction:** git write ops are client-initiated
  workspace writes, exactly like `POST /file`, `POST /rename`, `POST /mkdir`
  — they go through the same authenticated paired-device gate the protected
  router already enforces. They do **not** use the permission sink, which is
  session-scoped and reserved for **agent-initiated** ACP
  `session/request_permission` prompts.
- Revision guard: `POST /commit` requires `If-Match: <head-oid>` header
  (client sends the `head_oid` from the last `/status`). Mismatch → 409 with
  a clear message; client refetches status.
- Diff bounding: per-file cap (configurable in `config.toml`, default
  1 MiB). Truncation flagged in the response.
- Logging: every write op logs caller id, workspace, op, and outcome via
  `tracing`. Reads log at debug.

## Out of scope

- Branch operations (checkout, create, delete, merge).
- Stash, blame, log history browsing.
- Submodule diffing (surface as "not supported" in `/status`).
- Pull/fetch.

## Acceptance

- [ ] All endpoints behave per the table above; unauthenticated → 401.
- [ ] Path containment: a `path` containing `..` or resolving outside the
      workspace root is 400 and logged.
- [ ] Diff bounding truncates at the configured cap and sets `truncated`.
- [ ] `POST /commit` returns 409 on `If-Match` mismatch.
- [ ] `POST /push` streams stderr verbatim and never stores credentials.
- [ ] Authenticated paired-device gate on every endpoint (no permission sink
      — git writes are client-initiated, matching file-save/rename/mkdir).
- [ ] Unit + contract tests for each endpoint.

## Verification

- `cargo test -q --all-targets` with fixtures built via the `git` CLI.
- New contract fixtures in `tests/contract/` covering status/diff/stage/
  commit/push/init and the 409 / 400 / 401 paths.
- `make test-contract` green.

Suggested commit: `feat(git): backend status/diff/stage/commit/push API (S-GIT-API)`
