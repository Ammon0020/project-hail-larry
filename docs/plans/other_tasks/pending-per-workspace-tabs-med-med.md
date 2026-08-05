# Per-workspace tab persistence on server

> **Status:** pending | **Difficulty:** medium | **Urgency:** medium
> **Source:** user-noted improvements — UI section

## Goal

When switching workspaces, tabs should switch to that workspace's open tabs.
When reopening a workspace, its tabs restore. Tabs are remembered per-workspace
on the server side.

## Follow-up

After server-side persistence: sync tabs between devices via workspace
syncing (saved to workspace rather than browser), but only when workspace
syncing is enabled. Add a button to toggle workspace syncing on/off.

### Cross-device sync events needed

The existing event bus already broadcasts `SessionCreated` and `SessionClosed`
to all connected WebSocket clients (see `useBackend.ts` lines 339-358). For
full cross-device sync, add these new event types to `EventPayload` /
`EventType` in `src/interfaces/types.rs`:

- **`SessionRenamed { name: String }`** — emitted by `registry.rs::rename()`.
  Frontend handler: refresh session list (like `SessionCreated`). This also
  fixes the auto-naming same-device refresh gap (currently worked around with
  a `loadSessions()` call in `useSessionActions.ts::sendPrompt`).
- **`TabOpened { tabId: String, workspaceId: String }`** — new tab opened on
  another device. Frontend: open as background tab.
- **`TabClosed { tabId: String }`** — tab closed on another device.
- **`TabReordered { tabIds: String[], workspaceId: String }`** — tab order
  changed on another device.

Pattern to follow: see how `SessionCreated`/`SessionClosed` are wired through
`EventPayload` → `EventType` → `append_payload()` → WS broadcast →
`useBackend.ts` event handler → `pendingCreatedSessionIds` queue →
`ChatPanel.tsx` drain effect.

## Acceptance

- [ ] Tabs are persisted per-workspace on the server
- [ ] Switching workspace switches which tabs are open
- [ ] Reopening a workspace restores its tabs
- [ ] (Follow-up) Tabs sync between devices when workspace syncing enabled
- [ ] (Follow-up) Button to toggle workspace syncing on/off
- [ ] `make check` passes
