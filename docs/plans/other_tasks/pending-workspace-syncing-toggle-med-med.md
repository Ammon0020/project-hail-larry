# Workspace syncing toggle — per-workspace tab sync between devices

> **Status:** pending | **Difficulty:** medium | **Urgency:** medium
> **Source:** user-noted improvements — UI section
> **Parent:** `pending-user_noted_improvements-large-high.md`

## Goal

Add a per-workspace "workspace syncing" toggle that controls whether editor
tabs are synced between devices via the server. When enabled, tabs are saved to
the workspace (server-side) and shared across devices. When disabled, tabs stay
browser-local only.

## Scope

- **Backend config**: Add a `sync_tabs` (or `workspace_syncing`) boolean to the
  workspace config model, defaulting to `true` (current behavior). Expose via
  `PATCH /api/workspaces/{id}` or a dedicated toggle endpoint.
- **Tab sync behavior**: When `sync_tabs` is `false`, the PUT/GET
  `/api/workspaces/{id}/tabs` endpoints either no-op (server returns empty) or
  the frontend skips the server round-trip and uses localStorage only. Pick the
  simpler approach — frontend skip is cleaner (no backend behavioral fork).
- **UI toggle**: A button in `WorkspaceHeader.tsx` (next to the online
  indicator) to turn workspace syncing on/off. Visual state should be clear
  (icon + tooltip).
- **Out of scope**: Cross-device file content sync (tabs are identity/order
  only; content is always local).

## Dependencies

- Per-workspace tab persistence (done — `d8e6cd3`).

## Acceptance

- Workspace config has a `sync_tabs` field, persisted and replayed.
- UI toggle in the workspace header reflects and controls the setting.
- When off, tabs are browser-local; when on, tabs sync to the server.
- `make check` passes.

## Verification

```text
make check
```
