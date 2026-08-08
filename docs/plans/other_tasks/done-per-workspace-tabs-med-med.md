# Per-workspace tab persistence on server

> **Status:** done | **Difficulty:** medium | **Urgency:** medium
> **Source:** user-noted improvements — UI section
> **Completed:** 2026-08-06 — commit `d8e6cd3`

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

## What shipped

Server stores tab **identity and order only** — never file content. Unsaved
buffers are drafts belonging to the device that typed them; syncing them would
let one device silently overwrite another's edits. That split is also what
makes the cross-device follow-up safe to build.

- `src/workspace/tabs.rs` — `TabStore` over `~/.local-agent/workspace-tabs.json`
  (atomic write, 0600, same pattern as `ConversationStore`). Deliberately *not*
  in `config.toml`: tab layout churns on every open/close, and that file holds
  TLS and trust settings.
- `GET`/`PUT /api/workspaces/{id}/tabs`. Unknown workspace → 404 so tabs cannot
  accumulate against phantom ids. Payload capped at
  `MAX_TABS_PER_WORKSPACE` (200) and 1024 chars per field — a paired device is
  otherwise unbounded in how large it can make the state file.
- `web/src/lib/workspaceTabs.ts` — pure resolve/serialize logic, unit-tested.
  An empty server record is treated as "no record", not "no tabs", so the first
  run after upgrade adopts the device's existing layout instead of wiping it.
- Content for tabs restored without a local buffer is read from disk; tabs whose
  file no longer reads are dropped rather than left as empty buffers that could
  be saved over the real file.
- A device-local draft stash (`lai:tabDrafts`) holds the **unsaved** buffers of
  workspaces that are off screen. Switching replaces the whole open-tab set, so
  without it the outgoing workspace's buffers left React state and the switch
  back re-read from disk — silently discarding typing. Only unsaved tabs are
  kept (a clean tab's content is on disk, so re-reading is fresher), the stash
  is recomputed on every departure so a saved tab's draft cannot linger and
  overwrite it later, and it is capped at `MAX_DRAFT_WORKSPACES` (8) because
  drafts carry full file content.
- The settings tab is carried across a workspace switch: it belongs to no
  workspace, so swapping the tab set should not close it.

## Acceptance

- [x] Tabs are persisted per-workspace on the server
- [x] Switching workspace switches which tabs are open
- [x] Reopening a workspace restores its tabs
- [ ] (Follow-up) Tabs sync between devices when workspace syncing enabled → *`pending-workspace-syncing-toggle-med-med.md`*
- [ ] (Follow-up) Button to toggle workspace syncing on/off → *`pending-workspace-syncing-toggle-med-med.md`*
- [x] `make check` passes
- [x] (2026-08-06) `TabStore::remove` called on workspace deletion; orphaned tab entries no longer outlive their workspace.
- [x] (2026-08-06) Workspace-switch logic extracted into pure `planWorkspaceSwitch` for unit testing.
