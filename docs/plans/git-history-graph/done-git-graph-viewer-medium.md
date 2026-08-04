# S-GIT-GRAPH-VIEWER — Commit graph component

> **Status:** Done (2026-07-29). **Difficulty:** medium. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** S-GIT-LOG-API (needs the log endpoint).
>
> *Done 2026-07-29 — `web/src/components/git/GitHistorySection.tsx` renders a
> collapsible, resizable bottom pane inside the Source Control panel with an SVG
> commit graph (one column per branch, dots for commits, curved/straight edges
> for parents) plus a graph/flat-list toggle. Lane layout lives in the pure
> `web/src/components/git/gitGraphLayout.ts` (gitk/tig-style greedy lane
> assignment, offscreen-edge stubs for paginated-out parents) with 6 vitest
> cases in `gitGraphLayout.test.ts`. Rows are virtualized with
> `@tanstack/react-virtual` and append more pages via `Load more history`
> (`getGitLog(limit=100, offset=commits.length)`). Clicking a row opens
> `GitCommitDiffTab.tsx`, a read-only multi-file diff tab reusing
> `GitDiffViewer`, backed by the new `GET /api/workspaces/{id}/git/commit-diff?
> oid=` endpoint (`commit_diff()` in `src/git/worktree.rs`, `CommitDiffResult`
> / `CommitDiffFile` in `src/git/types.rs`, `get_git_commit_diff` handler +
> route in `src/api/{git,mod}.rs`, 2 unit tests in `src/git/tests.rs`). The
> pane is wired from `GitPanel.tsx` via `onOpenCommitDiff` and surfaced as a
> tab in `App.tsx` / `EditorPane.tsx`. `make check` passes.*

## Goal

A commit history surface in the Source Control panel: an SVG commit graph with
branch/merge topology, clickable rows that open the commit diff, and a flat
list fallback for narrow viewports.

## Scope

- **Bottom pane** in `GitPanel`: collapsible (chevron header) and resizable
  (pointer-drag separator, 160–480 px, arrow-key nudges). Defaults to 260 px.
- **SVG commit graph**: one column per branch lane, dots for commits, straight
  lines for first-parent edges, curved Bézier paths for branch/merge edges,
  dashed stubs for offscreen (paginated-out / root) parents. HEAD dot is
  ringed. Layout computed by `layoutGitGraph` (greedy lane assignment: first
  parent reuses the commit's lane, additional parents fan out to free lanes;
  lanes free once their commit is drawn and no parent was placed on them).
- **Row content**: graph segment + short hash + message (truncated) + relative
  time + branch labels as colored chips.
- **Graph / list toggle**: header button switches between SVG graph and flat
  list (no graph column) for narrow/mobile viewports.
- **Virtualization**: `@tanstack/react-virtual` with 44 px rows and 8-row
  overscan; only visible rows render.
- **Pagination**: initial `getGitLog()` fetch, then `Load more history` appends
  the next 100 commits (dedup by oid) when `hasMore` is set.
- **Commit diff tab**: `GitCommitDiffTab` opens on row click, lists changed
  files in a side nav, and renders the selected file via `GitDiffViewer`
  (parent vs commit snapshot). Reuses the existing diff tab infrastructure.
- **Backend**: `GET /api/workspaces/{id}/git/commit-diff?oid=` returns
  `CommitDiffResult { oid, parentOid, files[] }` where each file carries its
  parent (`base`) and commit (`head`) snapshots plus the shared `DiffResult`
  fields. Uses validated commit ids with bounded Git CLI ref/tree reads.

## Out of scope

- Tag chip colors (S-GIT-TAGS).
- Search/filter bar.
- Interactive rebase or cherry-pick.

## Acceptance

- [x] Commit history visible as an SVG graph with branch/merge topology.
- [x] History pane is collapsible and resizable (pointer drag + arrow keys),
      sitting below the current-changes sections in the Source Control panel.
- [x] Graph / flat-list toggle switches the row rendering for narrow viewports.
- [x] Rows are virtualized (`@tanstack/react-virtual`) and scale to 1000+
      commits; `Load more history` paginates further pages.
- [x] Clicking a commit opens `GitCommitDiffTab` showing the commit's
      parent-vs-commit diff via `GitDiffViewer`.
- [x] `GET /api/workspaces/{id}/git/commit-diff?oid=` returns the commit's
      changed files with parent and head snapshots; invalid oids error cleanly.
- [x] `layoutGitGraph` covered by unit tests (linear, offscreen parent, side
      branch + merge, determinism, empty log, octopus merge).
- [x] `make check` passes (fmt + clippy + test + frontend + contract).
