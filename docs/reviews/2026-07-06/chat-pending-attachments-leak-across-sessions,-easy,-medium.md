# Pending attachments leak across session switches, "New Chat", and session-gone errors

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `web/src/components/ChatPanel.tsx`
- **Lines:** 267-278 (handleSend catch), 320-327 (handleFileChange catch), 392-399 (handleNewChat); also no cleanup effect on activeSessionId change

## Description

`pendingAttachments`/`pendingPreviews` are only cleared on successful send (lines 265-266) and on explicit per-index remove (335-336). They are NOT cleared when: (a) the user clicks "New Chat" (`handleNewChat` only resets `input`/`error` and calls `onSelectSession('')`); (b) a send or upload fails with a "session gone" error and `onSelectSession('')` resets to new-chat state (lines 272-274, 322-324); (c) the user picks a different session in `ChatHistory` (no effect clears them when `activeSessionId` changes). Each uploaded file is stored server-side under `/api/sessions/:sessionId/uploads/:id` for the session it was uploaded to. After any of the above state transitions, the persisted preview chips and `pendingAttachments` ids reference uploads belonging to a now-inactive or deleted session. When the user subsequently sends in a different/new session, `onSendMessage` is called with attachment ids that do not exist for that session — the backend will either reject the prompt or silently drop the images, and the user sees stale thumbnails pointing at dead URLs.

## Recommendation

Clear `pendingAttachments` and `pendingPreviews` in `handleNewChat`, in both `isSessionGone` catch branches, and in a `useEffect` keyed on `activeSessionId` (e.g. `useEffect(() => { setPendingAttachments([]); setPendingPreviews([]) }, [activeSessionId])`). Revoking is not needed since `result.url` is a server URL, not a blob URL.

## Verification

Grepped `setPendingAttachments|setPendingPreviews|handleNewChat` — only 4 production call sites: init (116-117), send-success clear (265-266), append (314-318), filter-remove (335-336). `handleNewChat` (392-399) contains no pending-state reset. Both `isSessionGone` branches (272-274, 322-324) call `onSelectSession('')` without clearing pending state. No `useEffect` depends on `activeSessionId` for cleanup. Confirmed `UploadResult.url`/`Attachment.uri` resolve to `/api/sessions/:id/uploads/:id` (api.ts:244, ChatMessageItem.tsx:137), so attachments are session-scoped.
