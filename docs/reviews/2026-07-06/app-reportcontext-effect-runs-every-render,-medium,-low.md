# reportContext effect runs on every render because `backend` is in deps and is recreated each render

- **Difficulty:** medium
- **Urgency:** low
- **File:** `web/src/App.tsx`
- **Lines:** 210-215

## Description

The effect that reports open files / recent edits to the backend lists `[openTabs, activeSessionId, backend]` as deps. useBackend returns a fresh object literal every render (useBackend.ts lines 487-524, no useMemo/useCallback on the returned object or its methods), so `backend` identity changes every render and the effect fires every render. Each fire calls backend.reportContext, which clears and reschedules a 1s debounce timer (useBackend.ts lines 447-456). During active event streaming (frequent WebSocket events → frequent re-renders) the timer is reset faster than 1s, so the context report may never actually fire until streaming stops. The file-change effect (line 311) intentionally keys on `[backend.events]` to avoid exactly this pattern; this effect did not get the same treatment.

## Recommendation

Drop `backend` from the dep array and disable exhaustive-deps with the same justification used at line 188/310, OR extract the stable bits (backend.reportContext, backend.activeWorkspace) via refs. Keying on `[openTabs, activeSessionId, backend.activeWorkspace]` plus a ref to reportContext would run only when the inputs that matter actually change.

## Verification

Read App.tsx lines 210-215 (deps include `backend`). Read useBackend.ts lines 487-525 (return is a plain object literal, not memoized). Read useBackend.ts lines 447-456 (reportContext clears+resets the timer on every call). Compare with App.tsx line 311 where the file-change effect deliberately keys on `[backend.events]` to avoid the same trap.
