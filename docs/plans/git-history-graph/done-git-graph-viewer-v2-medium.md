# S-GIT-GRAPH-VIEWER-V2 — VS Code-style graph viewer

> **Status:** Done. **Difficulty:** medium. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-viewer-medium.md`, `done-git-commit-diff-status-small.md`.

## Goal

Upgrade the basic graph viewer to a VS Code-style experience: per-row SVG with
edge continuity, semantic colors, inline expansion, and a context menu.

## Scope

- Per-row SVG with edge continuity (active-lane verticals, merge curves).
- Per-lane color palette (8 semantic token colors cycled by lane).
- Inline expansion on click: shows changed files with A/M/D/R status badges
  (`CommitFileList.tsx`, `CommitStatusBadge.tsx`).
- Right-click context menu: Copy SHA, Open diff in tab, Checkout, Refresh
  (`CommitContextMenu.tsx`).
- HEAD ring indicator and merge dot sizing.
- Pure SVG segment builder (`gitGraphSvg.ts`) — testable, no React coupling.

## Acceptance

- [x] Graph renders per-row SVG with continuous lane verticals and merge curves.
- [x] Lanes use the semantic token palette, cycled by lane index.
- [x] Clicking a commit expands an inline file list with status badges.
- [x] Right-click opens a context menu with Copy SHA / Open diff / Checkout /
      Refresh.
- [x] HEAD commit shows a ring indicator; merge commits render a larger dot.
- [x] `gitGraphSvg.ts` is a pure module with unit tests (no React imports).
- [x] `make check` passes.

## Status note

Done in this wave. Supersedes the original `done-git-graph-viewer-medium.md`
viewer; the segment builder was extracted into a pure module to keep it
testable and to set up the edge-driven refactor (S-GIT-GRAPH-EDGE-RENDER).
