# S-GIT-LOG-PERF — Backend log pagination performance

> **Status:** Pending. **Difficulty:** medium. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-log-api-medium.md`.

## Goal

Stop decoding author/message metadata for every reachable commit before
applying offset/limit, so per-page loads stay cheap on large repos.

## Scope

- Apply offset/limit during the walk, not after collecting all commits.
- Still compute accurate `total` and `has_more` without decoding skipped
  commits.
- Consider `gix` rev-walk with bounded metadata decoding (decode only the
  commits that survive the offset/limit window).

## Acceptance

- [ ] `GET .../git/log?offset=N&limit=L` decodes only the returned commits'
      metadata, not all reachable commits.
- [ ] `total` and `has_more` remain accurate.
- [ ] Page load on a 10k-commit repo stays under the existing budget (no
      regression vs. the current path on small repos, large improvement on
      big repos).
- [ ] `make check` passes.
