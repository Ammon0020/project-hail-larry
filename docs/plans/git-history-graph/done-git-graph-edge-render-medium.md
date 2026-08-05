# S-GIT-GRAPH-EDGE-RENDER — Edge-driven graph rendering

> **Status:** Done. **Difficulty:** medium. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-viewer-v2-medium.md`.

## Goal

Refactor graph rendering from lane-set-difference inference to edge-driven
segment generation, fixing missing convergence curves and offscreen stub
direction.

## Scope

- `gitGraphLayout.ts` emits a `ParentEdge` discriminated union
  (`visible | truncated`) with assigned lanes for truncated parents.
- `gitGraphSvg.ts` renders one outgoing segment per parent edge (no more
  lane-set-difference inference).
- Truncated parents get an assigned lane so offscreen stubs render downward,
  not upward.
- Convergence curves (side branch → main lane) now render correctly.

## Acceptance

- [x] Convergence regression test passes (side branch merges back to main lane
      with a curve).
- [x] Edge invariant test passes (one outgoing segment per parent edge).
- [x] Truncation direction test passes (offscreen stubs point downward).
- [x] Truncated merge test passes (merge with a truncated parent renders
      correctly).
- [x] `make check` passes.

## Status note

Done in this wave. The edge-driven model is the foundation for stable branch
colors (S-GIT-GRAPH-COLORS), which will key color off `ParentEdge` lineage
instead of lane index.
