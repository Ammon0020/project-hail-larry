# S-GIT-GRAPH-COLORS — Stable branch colors

> **Status:** Pending. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-edge-render-medium.md`.

## Goal

Color graph edges and verticals by branch lineage instead of lane index, so
colors stay stable when lanes are reused and across pagination appends.

## Scope

- Add a `lineageId` / `colorKey` to `ParentEdge` in the layout, derived from
  the branch ref or the first commit on that lineage.
- Color edges and verticals by lineage, not lane index.
- Keep colors stable across pagination appends (same lineage → same color).

## Acceptance

- [ ] Two commits on the same branch share a color even when their lane index
      differs.
- [ ] Appending a new page does not recolor existing rows.
- [ ] `make check` passes.
