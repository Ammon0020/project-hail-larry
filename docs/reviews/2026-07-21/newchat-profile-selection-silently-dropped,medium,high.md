# New-chat profile selection is silently dropped on first send

- **Difficulty:** medium
- **Urgency:** high
- **File:** `/media/adam/extex/projects/project-hail-larry/web/src/components/ChatPanel.tsx`
- **Lines:** 243-272 (handleProfileChange), 414-448 (handleSend)

## Description

`handleProfileChange` has a special "no live session yet" branch (lines 245-249) that stores the user's pick as `profileOverride = { sessionId: '', profileId }` with the comment "it gets pushed to the backend when the session is created on first send." That push never happens.

`handleSend` (lines 414-448) creates the new session via `onCreateSession` and then calls `onSendMessage(sessionId, content, attachmentsToSend...)`. The `profile` parameter was removed from `onSendMessage` / `api.sendPrompt` / `useBackend.sendPrompt` in this same diff, so there is no longer any code path that applies a profile to a freshly-created session. Once `activeSessionId` becomes the new session id, the `selectedProfileId` memo (lines 205-218) no longer matches the `sessionId: ''` override and falls back to `profileConfig.defaultProfileId` (no localStorage entry exists for the new session yet). The dropdown therefore reverts to the default profile visually, and the backend never receives `POST /sessions/:id/profile` for the user's chosen profile — the first prompt runs against the default profile regardless of what the user picked before sending.

The comment in `handleProfileChange` promises behavior that does not exist, which makes the bug easy to miss on a casual read.

## Recommendation

After `onCreateSession` returns in `handleSend` (around line 426), if a `profileOverride` with `sessionId: ''` is present, call `setSessionProfile(sessionId, profileOverride.profileId)`, persist it to `localStorage.setItem(\`local-agent:profile:${sessionId}\`, profileOverride.profileId)`, and update `profileOverride` to `{ sessionId, profileId }` so the dropdown stays in sync. Clear the empty-session override once consumed. Alternatively, restore the `profile` argument on `sendPrompt`/`onSendMessage` and have the backend apply it on session creation. Either way, remove or correct the misleading comment.

## Verification

Read `handleSend` (lines 414-467): after `sessionId = await onCreateSession(...)` there is no `setSessionProfile` call and `onSendMessage` no longer accepts a profile argument (confirmed in `App.tsx` line 858 `await backend.sendPrompt(sessionId, content, attachments)` and `useBackend.ts` line 519). The `selectedProfileId` memo only returns the override when `profileOverride.sessionId === activeSessionId`; after session creation `activeSessionId` is the new id while the override still holds `''`, so the override is ignored.
