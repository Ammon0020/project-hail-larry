# Silent file-refresh in file-change effect omits workspaceId

- **Difficulty:** easy
- **Urgency:** high
- **File:** `web/src/App.tsx`
- **Lines:** 294-308

## Description

The live file-change effect silently reloads clean tabs from disk when an agent writes a file or an external edit is detected. The reload loop calls `backend.readFile(tab.path)` without passing `tab.workspaceId`. The same diff explicitly added `tab.workspaceId` to `handleSave` (line 341) and `handleReloadTab` (line 363) precisely because a tab opened from workspace A must be read/saved against workspace A's root even after the active workspace switches to B. This third call site was missed. If the user has switched workspaces since opening the tab, this read goes to the wrong workspace root (or fails with "no such file"), and the subsequent setOpenTabs update either silently no-ops or clobbers the tab with the wrong file's content. The Tab type comment (types/index.ts lines 30-36) documents exactly this hazard.

## Recommendation

Pass the tab's workspace id: `backend.readFile(tab.path, tab.workspaceId)`. Mirror the fix already applied in handleSave/handleReloadTab.

## Verification

Read App.tsx lines 294-308 (calls `backend.readFile(tab.path)` with one arg) vs lines 341 and 363 (call with `tab.workspaceId`). Read useBackend.ts lines 380-383 confirming readFile signature `(path, workspaceId?)` falls back to activeWorkspace when unset. Read types/index.ts lines 30-36 documenting the workspaceId rationale. Independently re-verified by reading App.tsx lines 290-314.
