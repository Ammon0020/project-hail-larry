# Send button / canSend not gated on uploading — race condition and duplicate-session risk

- **Difficulty:** easy
- **Urgency:** high
- **File:** `web/src/components/ChatPanel.tsx`
- **Lines:** 354-359 (also 246-248 handleSend guard, 631-638 send button)

## Description

`canSend` and the `handleSend` early-return guard check `sending` but not `uploading`. While `handleFileChange` is mid-flight (the paperclip shows a spinner, the upload button is disabled), the textarea Send button remains enabled and Enter still triggers `handleSend`. Two concrete consequences: (a) the user can send a prompt before in-progress uploads have been appended to `pendingAttachments`, so the message goes out without the images they are still attaching; (b) in the "new chat" state, both `handleFileChange` and `handleSend` independently call `onCreateSession` (`App.tsx:497` `handleCreateSession` → `backend.createSession` + `setActiveSessionId`), so two sessions can be created concurrently — the attachment is uploaded to one session while the prompt is sent to a different one, orphaning the upload. `handleKeyDown` (line 347-352) also calls `handleSend` unconditionally on Enter, bypassing the disabled-button protection entirely.

## Recommendation

Add `!uploading` to `canSend` and to the `handleSend` guard (`if ((!content && pendingAttachments.length === 0) || sending || uploading || ...)`), and/or disable the textarea / ignore Enter while `uploading` is true.

## Verification

Read `canSend` (lines 354-359) — it references `sending` only. Read `handleSend` guard (line 248) — same. Read `handleKeyDown` (347-352) — calls `handleSend` with no `uploading` check. Read `handleFileChange` (298-331) — sets `uploading` but `handleSend` is unaware. Confirmed `onCreateSession` maps to `App.tsx:handleCreateSession` which creates a backend session and sets it active, so two concurrent calls produce two sessions.
