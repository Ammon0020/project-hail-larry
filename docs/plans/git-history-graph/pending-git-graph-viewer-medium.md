# S-GIT-GRAPH-VIEWER — Commit graph component

> **Status:** Pending. **Difficulty:** medium. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** S-GIT-LOG-API (needs the log endpoint).

## Goal

New `web/src/components/git/GitHistoryTab.tsx` rendering an SVG commit graph
with branch/merge topology, clickable rows that open the commit diff.

## Scope

- SVG commit graph: one column per branch, dots for commits, curved lines for
  parent edges. Column assignment computed from the parent graph (greedy:
  assign each commit to its parent's column, or a new column for branch points).
- Each row: graph segment + short hash + message (truncated) + relative time +
  branch labels as colored chips.
- Click a commit → open `GitDiffViewer` with the commit's diff (parent vs
  commit). Reuses the existing diff tab infrastructure.
- Virtualized list for performance (react-window or intersection-observer
  lazy-render).
- Mode toggle: graph vs. flat list (for narrow mobile viewports).
- Reached from a "History" button in the GitPanel header.

## Out of scope

- Tag chip colors (S-GIT-TAGS).
- Search/filter bar.
- Interactive rebase or cherry-pick.

## Acceptance

- [ ] Commit history visible as a graph with branch topology.
- [ ] Clicking a commit opens its diff in `GitDiffViewer`.
- [ ] Graph virtualizes for repos with 1000+ commits.
- [ ] Mobile: graph collapses to flat list on narrow viewports.
- [ ] `make check` passes.
