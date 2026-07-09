# uploadFile added to api but not surfaced through useBackend hook

- **Difficulty:** easy
- **Urgency:** high
- **File:** `web/src/hooks/useBackend.ts`
- **Lines:** 487-524 (return object)

## Description

The diff adds `api.uploadFile` (api.ts:241-253) but `useBackend`'s returned object does not include an `uploadFile` wrapper. `ChatPanel.tsx:4` imports `api` directly and calls `api.uploadFile(sessionId, file)` at line 313, bypassing the hook. This violates the project's architecture: `useBackend` is the documented backend interface (every other action — `sendPrompt`, `readFile`, `saveFile`, `createSession`, etc. — is wrapped and exposed by the hook). Consequences: (a) uploads skip the hook's session-not-found recovery that `sendPrompt` implements (useBackend.ts:398-416), so a stale `activeSessionId` during upload throws a raw 404 instead of the friendly reset; (b) `useMockBackend` has no `uploadFile` equivalent, so any component using the mock cannot exercise the upload path; (c) error/loading state for uploads can't be centrally tracked.

## Recommendation

Add an `async function uploadFile(sessionId: string, file: File)` to `useBackend` that wraps `api.uploadFile` with the same stale-session recovery as `sendPrompt` (detect `isSessionNotFound`, clear `lai:activeSessionId`, throw friendly error), and add it to the returned object. Mirror it in `useMockBackend` (e.g. return a fake `UploadResult` with a blob URL).

## Verification

`grep` for `uploadFile` shows the only call site is `ChatPanel.tsx:313` reaching into `api` directly; the return object in `useBackend.ts:487-524` has no `uploadFile` key; `useMockBackend.ts` returns only `{ events, sendPrompt, respondPermission }`.
