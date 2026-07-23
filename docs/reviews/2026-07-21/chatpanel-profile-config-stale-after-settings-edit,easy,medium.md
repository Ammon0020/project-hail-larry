# ChatPanel's profile config is never refreshed after Settings edits

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `/media/adam/extex/projects/project-hail-larry/web/src/components/ChatPanel.tsx`
- **Lines:** 220-236

## Description

`ChatPanel` fetches `GET /api/profiles` exactly once on mount (lines 222-236) and stores it in `profileConfig`. `ProfilesSettings` saves edits via `PUT /api/profiles` but emits no event and does not signal `ChatPanel` to reload. The composer dropdown therefore keeps showing stale labels, and a profile renamed or deleted in Settings is still offered in the chat selector.

Worse, if the user deletes a profile in Settings that a chat session had selected, `handleProfileChange` in `ChatPanel` will call `setSessionProfile(activeSessionId, <deleted id>)` only if the user re-selects it — but the persisted localStorage value for a session may reference a now-deleted id, and the dropdown's `selectedProfileId` will resolve to that id with no matching option. The label fallback `?? selectedProfileId` (ChatComposer line 274) then renders a raw id, and any subsequent switch attempt targets a profile the backend no longer knows.

## Recommendation

Either expose a profile-changed signal (e.g. a `useBackend` event over the existing WebSocket sync, a `CustomEvent` on `window`, or a shared context/mutable ref that `ProfilesSettings` bumps on save and `ChatPanel` subscribes to) and re-run the `getProfiles()` fetch when it fires, or have `ChatPanel` reload profiles when the chat panel regains focus / the settings tab closes. At minimum, when `selectedProfileId` is not found in `profiles`, fall back to `profileConfig.defaultProfileId` and clear the stale localStorage entry instead of showing a raw id.

## Verification

`useEffect(() => { ... void getProfiles() ... }, [])` (lines 222-236) has an empty dependency array, so it runs only on mount. A repo search for `setProfileConfig` shows it is only set inside that effect (line 227) and never elsewhere. `ProfilesSettings.handleSave` (lines 207-216) calls `putProfiles(draft)` and `setSaved(draft)` but does not notify any other component.
