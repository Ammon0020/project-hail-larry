# Epic: Git History Graph, Branch Switching, and Remote Sync

> **Status:** Pending (7 of 14 stories done). **Difficulty:** large. **Urgency:** medium.
> **Source:** user request (2026-07-28). **Created:** 2026-07-28.
> **Depends on:** `done-git-action-bar-large.md` (git API + panel surface).

## Goal

Add a VS Code-style commit history graph with branch/merge topology, plus
branch switching and fetch/pull — the features that make the git panel feel
complete. The graph reuses the existing `GitDiffViewer` for per-commit diffs.

## Why now

The git panel handles status/stage/commit/push, but there's no way to:
- See commit history or branch topology
- Switch branches from the UI
- Fetch/pull from a remote

Users currently need a terminal for these, breaking the self-contained IDE
promise. The graph is the most-requested remaining git feature.

## Architecture decisions

- **Graph rendering:** SVG-based commit graph (not a third-party library).
  The topology is simple enough — vertical column per branch, dots for
  commits, lines for parent edges. Keeps the bundle small and the rendering
  controllable. Compare: git-log-graph libs (e.g. `gitgraph.js`) add ~50KB
  and have stale maintenance; a custom SVG is ~200 lines and matches the
  app's token system.
- **Edge-driven rendering:** the layout emits a `ParentEdge` discriminated
  union (visible | truncated) and the SVG builder renders one outgoing
  segment per parent edge. This replaced the earlier lane-set-difference
  inference and fixed missing convergence curves and offscreen stub
  direction. See `done-git-graph-edge-render-medium.md`.
- **History data:** backend endpoint `GET /api/workspaces/{id}/git/log`
  returns a paginated commit list with parent refs, branch labels, and HEAD.
  The frontend computes the column layout from the parent graph. No `git log
  --graph` parsing — the topology is derived from parent SHAs, which is more
  reliable than parsing ASCII graph output.
- **Branch switching:** `POST /api/workspaces/{id}/git/checkout` with branch
  name. Refuses if working tree is dirty (returns 409 with the dirty file
  list). Detached-HEAD checkout of a specific SHA is a separate endpoint
  (pending, S-GIT-DETACHED-CHECKOUT).
- **Fetch/pull:** `POST /api/workspaces/{id}/git/fetch` and `.../git/pull`.
  Streams stderr like the existing push handler. No credential storage —
  uses the agent's environment git credentials (same model as push).
- **Virtualization:** the graph list virtualizes rows to handle repos with
  thousands of commits without rendering all rows.

## Stories

### Done

- **S-GIT-LOG-API** — `done-git-log-api-medium.md`. Backend
  `GET /api/workspaces/{id}/git/log` returns paginated commits with parents,
  author, message, branch labels, and `is_head`. Uses `gix` rev-walk seeded
  from local branch heads + detached HEAD.
- **S-GIT-GRAPH-VIEWER** — `done-git-graph-viewer-medium.md`. Initial SVG
  commit graph component with virtualized rows, branch chips, and click-to-
  diff. Superseded by v2.
- **S-GIT-CHECKOUT** — `done-git-checkout-small.md`. Branch switching via
  `POST .../git/checkout`; searchable Radix branch picker in the GitPanel
  header; dirty-tree refusal (409).
- **S-GIT-FETCH-PULL** — `done-git-fetch-pull-small.md`. `POST .../git/fetch`
  and `.../git/pull` with stderr streaming; ahead/behind chips populated.
- **S-GIT-COMMIT-DIFF-STATUS** — `done-git-commit-diff-status-small.md`.
  `commit_diff()` uses `git diff-tree --name-status -r -z -M`; `CommitDiffFile`
  gains `status` and `old_path` (frontend mirrors with `status`/`oldPath`).
- **S-GIT-GRAPH-VIEWER-V2** — `done-git-graph-viewer-v2-medium.md`. VS
  Code-style upgrade: per-row SVG with edge continuity, per-lane color
  palette, inline expansion with A/M/D/R status badges, right-click context
  menu (Copy SHA / Open diff / Checkout / Refresh), HEAD ring, merge dot
  sizing, pure `gitGraphSvg.ts` segment builder.
- **S-GIT-GRAPH-EDGE-RENDER** — `done-git-graph-edge-render-medium.md`.
  Refactored rendering to edge-driven segment generation via a `ParentEdge`
  discriminated union; fixes convergence curves and offscreen stub
  direction.

### Pending

- **S-GIT-DETACHED-CHECKOUT** — `pending-git-detached-checkout-small.md`.
  `POST .../git/checkout-commit` for detached-HEAD checkout of any SHA;
  context menu Checkout enabled for all commits; "detached HEAD" indicator.
- **S-GIT-CONTEXT-MENU-KB** — `pending-git-context-menu-kb-small.md`.
  Replace the custom context menu with `@radix-ui/react-context-menu` for
  built-in arrow-key navigation, focus management, and Escape-to-close.
- **S-GIT-GRAPH-FILTER** — `pending-git-graph-filter-medium.md`. Search bar
  in the graph pane header filtering by author, message substring, and SHA
  prefix; debounced; "X of Y commits" count; clear button.
- **S-GIT-SCROLL-TO-HEAD** — `pending-git-scroll-to-head-small.md`. "Scroll
  to HEAD" button that virtualizer-scrolls to the HEAD row, fetching pages
  if needed, and briefly highlights it.
- **S-GIT-DYNAMIC-EXPANSION** — `pending-git-dynamic-expansion-small.md`.
  Replace the fixed 160px expansion slot with `measureElement`-based dynamic
  height; re-measure on load; support multiple simultaneous expansions.
- **S-GIT-GRAPH-COLORS** — `pending-git-graph-colors-small.md`. Color edges
  and verticals by `ParentEdge` lineage id, not lane index, so colors stay
  stable across lane reuse and pagination appends.
- **S-GIT-LOG-PERF** — `pending-git-log-perf-medium.md`. Apply offset/limit
  during the rev walk so skipped commits aren't decoded; keep `total` and
  `has_more` accurate.
- **S-GIT-TAGS** — `pending-git-tags-small.md` (optional). Tag labels in the
  graph with a distinct chip color; display only.
- **S-GIT-STASH** — `pending-git-stash-small.md` (optional). Stash
  push/pop/drop/list with a GitPanel stash section.

## Out of scope

- Interactive rebase UI (very complex, high maintenance burden).
- Merge conflict resolution UI (complex, rare in this workflow).
- Force push / history rewriting.
- Cherry-pick / revert from graph.
- Blame (separate surface — editor integration, not git panel).
- Remote management (add/remove/edit remotes).
- PR/MR integration.

## Acceptance (epic-level)

Each story has its own acceptance criteria in its story file. Epic-level:
- [x] Commit history visible as a graph with branch topology.
- [x] Clicking a commit opens its diff.
- [x] Branch switching works from the UI.
- [x] Fetch/pull works and updates ahead/behind chips.
- [x] Graph virtualizes for repos with 1000+ commits.
- [x] Per-file A/M/D/R status shown on commit diffs and inline expansion.
- [x] Right-click context menu on commits (Copy SHA / Open diff / Checkout /
      Refresh).
- [ ] Detached-HEAD checkout from the context menu (S-GIT-DETACHED-CHECKOUT).
- [ ] Keyboard-navigable context menu (S-GIT-CONTEXT-MENU-KB).
- [ ] Filter/search the history by author, message, or SHA (S-GIT-GRAPH-FILTER).
- [ ] Scroll-to-HEAD button (S-GIT-SCROLL-TO-HEAD).
- [ ] Dynamic inline expansion height + multiple expansions
      (S-GIT-DYNAMIC-EXPANSION).
- [ ] Stable branch colors across pagination (S-GIT-GRAPH-COLORS).
- [ ] Backend log pagination doesn't decode skipped commits (S-GIT-LOG-PERF).
- [ ] Mobile: graph collapses to flat list on narrow viewports.
- [ ] `make check` passes for all stories.

## Suggested order

Done:
1. S-GIT-LOG-API (backend, no UI)
2. S-GIT-GRAPH-VIEWER (frontend, depends on log API)
3. S-GIT-CHECKOUT (small, high value)
4. S-GIT-FETCH-PULL (small, completes the remote loop)
5. S-GIT-COMMIT-DIFF-STATUS (small, unblocks v2 expansion badges)
6. S-GIT-GRAPH-VIEWER-V2 (medium, VS Code-style upgrade)
7. S-GIT-GRAPH-EDGE-RENDER (medium, edge-driven refactor + fixes)

Next (in suggested order):
8. S-GIT-DETACHED-CHECKOUT (small — unblocks Checkout on all commits)
9. S-GIT-CONTEXT-MENU-KB (small — a11y polish on the v2 menu)
10. S-GIT-SCROLL-TO-HEAD (small — quick UX win)
11. S-GIT-DYNAMIC-EXPANSION (small — fixes the fixed-height expansion slot)
12. S-GIT-GRAPH-COLORS (small — builds on the edge-driven layout)
13. S-GIT-GRAPH-FILTER (medium — client-side filter, no backend change)
14. S-GIT-LOG-PERF (medium — backend perf, independent of UI work)

Optional:
15. S-GIT-TAGS (optional)
16. S-GIT-STASH (optional)
