# S-GIT-LOG-API — Backend git log endpoint

> **Status:** Pending. **Difficulty:** medium. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-action-bar-large.md` (git API + `gix` infrastructure).

## Goal

`GET /api/workspaces/{id}/git/log?limit=100&offset=0` returns a paginated commit
list with parent refs, branch labels, and HEAD marker. Uses `gix` to walk the
commit graph (no `git log` CLI spawn).

## Response shape

```json
{
  "commits": [
    {
      "oid": "abc123",
      "parents": ["def456"],
      "message": "commit subject",
      "author": { "name": "...", "email": "...", "time": "2026-07-28T..." },
      "branch_labels": ["main"],
      "is_head": true
    }
  ],
  "total": 1234,
  "has_more": true
}
```

## Scope

- `limit` capped at 200; `offset` for pagination.
- Branch labels: resolve which branches point at each commit (scan refs).
- HEAD: mark the commit HEAD points at.
- New types + `log()` function in `src/git/mod.rs`; handler in `src/api/git.rs`;
  route in `src/api/mod.rs`.
- Tests following the existing `fresh_repo` + commit helper pattern.

## Out of scope

- Tag labels (separate story S-GIT-TAGS).
- Search/filter.
- `git log --graph` CLI parsing.

## Acceptance

- [ ] `GET /api/workspaces/{id}/git/log` returns commits with parents, author,
      message, branch labels, and `is_head`.
- [ ] `limit` capped at 200; `offset` paginates correctly.
- [ ] `total` and `has_more` populated.
- [ ] Unborn repo (no commits) returns empty list, not an error.
- [ ] `make check` passes (fmt + clippy + test + frontend + contract).
