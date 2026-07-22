# Story S-PROF-CHAT: Chat Profile Selector — Dynamic List + Per-Session Persistence

> **Status:** done | **Difficulty:** easy
> **Epic:** [profiles-over-acp](../pending-profiles-over-acp-hard.md).
> **Depends on:** S-PROF-REST, S-PROF-ACP.

## Goal

Populate the chat profile selector from configured profiles (not a hardcoded
list), persist the selection per session, and switch profiles via the new
`POST /sessions/:id/profile` endpoint instead of the removed `/prompt` body
field.

## Background / current behavior

- Selector: `web/src/components/ChatComposer.tsx:275-277` (hardcoded options).
- State: `web/src/components/ChatPanel.tsx:171` — NOT persisted.
- Currently sent as a `/prompt` body field via `web/src/lib/api.ts:325-332`;
  that field is removed in S-PROF-ACP.

## Desired behavior

- ChatComposer reads the profile list from `GET /api/profiles` (label + id),
  defaulting to `defaultProfileId`.
- ChatPanel persists the selected profile per session in `localStorage` keyed by
  session id; restores it on session open.
- Changing the profile calls `POST /sessions/:id/profile`; the `/prompt` call in
  `api.ts:325-332` no longer sends `profile`.

## Acceptance criteria

- [ ] Selector options come from `GET /api/profiles`; custom profiles appear
      without a code change.
- [ ] Selected profile persists across reload and is scoped per session id
      (switching sessions restores that session's last profile).
- [ ] Profile changes hit `POST /sessions/:id/profile`; `/prompt` payload no
      longer contains `profile` (matches S-PROF-ACP wire change).
- [ ] Default selection is `defaultProfileId` when no persisted value exists.
- [ ] `npm run build` + lint clean; `make test-contract` green (payload change).

## Out of scope

- Settings editor (S-PROF-UI); backend endpoint/removal (S-PROF-ACP, S-PROF-REST).
