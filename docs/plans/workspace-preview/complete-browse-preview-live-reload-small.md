# Story: Browse Preview Live Reload

> **Status:** complete | **Difficulty:** small
> **Epic:** [workspace-preview](../complete-workspace-preview-small.md). **Scope:**
> React frontend (reuse existing WS event stream).

## Goal

When a browse-preview tab is open and files change in that workspace (user save,
agent write, or external disk change), refresh the preview iframe automatically
so the user does not need to click Refresh.

## Background

- `BrowsePreview` already remounts the iframe via a `reloadKey` for manual refresh.
- `useBackend` delivers `FileWritten` / `FileChangedOnDisk` over the same WebSocket
  used for the file tree; `useFileChangeDetection` already consumes those for
  editor tabs.
- No second WS connection — pass `backend.events` into the preview.

## Desired behavior

- On `FileWritten` / `FileChangedOnDisk` for the preview's `workspaceId`, debounce
  ~250ms then bump `reloadKey` to remount the sandboxed iframe.
- Baseline event id on mount so historical events do not trigger a reload.
- MVP: any file in the workspace triggers reload (assets may live anywhere).

## Acceptance criteria

- [x] Preview remounts after save / agent write / on-disk change in its workspace.
- [x] Debounce coalesces bursty edits (~250ms).
- [x] No duplicate WebSocket; uses shared `backend.events`.
- [x] Manual Refresh still works.
- [x] `npm run build --silent` passes.

## Out of scope

- Dev-server proxy (Vite/Next.js).
- Path-scoped reload (only files under the entry directory).
- Mobile-specific preview UX.
