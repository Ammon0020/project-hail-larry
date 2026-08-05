# S-GIT-GRAPH-COLORS — Stable branch colors

> **Status:** Done (2026-07-30). **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-edge-render-medium.md`.
>
> *Done 2026-07-30 — `gitGraphLayout.ts` tracks a `lineageId` per branch lineage
> via a parallel `activeLineage` array. First parents continue the child's
> lineage; side parents start a new lineage; pre-placed parents inherit their
> lane's existing lineage (so convergence edges draw in the parent branch's
> color). `gitGraphSvg.ts` threads `lineageId` through `GraphVertical`,
> `GraphCurve`, and `GraphDot`. `GitHistorySection.tsx` colors by `lineageColor
> (lineageId)` instead of `laneColor(lane)`. 72 vitest cases pass; `make qcheck`
> passes.*

## Goal

Color graph edges and verticals by branch lineage instead of lane index, so
colors stay stable when lanes are reused and across pagination appends.

## Scope

- Add a `lineageId` / `colorKey` to `ParentEdge` in the layout, derived from
  the branch ref or the first commit on that lineage.
- Color edges and verticals by lineage, not lane index.
- Keep colors stable across pagination appends (same lineage → same color).

## Acceptance

- [x] Two commits on the same branch share a color even when their lane index
      differs.
- [x] Appending a new page does not recolor existing rows.
- [x] `make check` passes.
