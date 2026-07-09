# Uploads not cleaned up on daemon shutdown (resource/disk leak)

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `internal/daemon/daemon.go`
- **Lines:** 365-390

## Description

daemon.cleanup() tears down fsWatcher, server, syncHub, acpClient (CloseAllSessions), and eventStore, but never calls uploadsMgr.RemoveSession for any session. CloseAllSessions (acp.go:610) preserves session metadata in c.sessions on shutdown (intentional, so conversations survive restart), so the per-session upload directories under ~/.local-agent/uploads/{sessionID}/ are never deleted. The only cleanup path is handleCloseSession (api.go:561-563), which is the user-initiated DELETE route — it removes the session record AND the uploads. On a SIGINT/SIGTERM shutdown, every session's uploads are orphaned on disk forever. Over time this is an unbounded disk leak. The new uploadsMgr field is stored on the Daemon struct (line 140, 296) but is never read by cleanup() — it is dead state on the Daemon outside of construction.

## Recommendation

In cleanup(), after CloseAllSessions, iterate the known session IDs and call uploadsMgr.RemoveSession for each (best-effort, ignore errors). Either expose a method on acp.Client to list session IDs (ListSessions already exists per api.go:376), or have the daemon capture the session list before CloseAllSessions. Alternatively, document that uploads are intentionally retained across restarts (since conversations survive restart) and add a separate reaper/sweep for uploads whose session no longer exists in conversations.json — but the current code has neither cleanup nor a reaper.

## Verification

Read daemon.cleanup() (lines 365-390): it closes fsWatcher, server, syncHub, acpClient, eventStore — no uploads cleanup. Read acp.CloseAllSessions (acp.go:610-624): it deliberately preserves session metadata. Read handleCloseSession (api.go:549-566): uploads cleanup only happens on the DELETE route. Confirmed uploadsMgr is set on Daemon struct but unused in cleanup.
