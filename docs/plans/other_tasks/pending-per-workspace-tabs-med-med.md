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

## Acceptance

- [ ] Tabs are persisted per-workspace on the server
- [ ] Switching workspace switches which tabs are open
- [ ] Reopening a workspace restores its tabs
- [ ] (Follow-up) Tabs sync between devices when workspace syncing enabled
- [ ] (Follow-up) Button to toggle workspace syncing on/off
- [ ] `make check` passes
