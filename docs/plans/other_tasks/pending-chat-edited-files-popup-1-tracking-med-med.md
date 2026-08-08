# Chat edited-files popup — Story 1: edited-files tracking

> **Status:** pending | **Difficulty:** medium | **Urgency:** medium
> **Source:** user-noted improvements — Chat section
> **Parent:** `pending-user_noted_improvements-large-high.md`

## Goal

Track which files an agent has edited during a session and expose that list to
the frontend, so a follow-up story can render the popup UI.

## Scope

- **Backend**: The `FileWritten` event already fires when an agent writes a file
  via ACP `WriteTextFile`. Add a derived query (or event replay filter) that
  returns the set of files written in a session, with revision/change metadata.
  No new event type needed — aggregate from the existing event store.
- **Frontend**: Add a `useEditedFiles(sessionId)` hook that subscribes to the
  session event stream and maintains a list of edited files (path, line counts
  if available from the diff, timestamp). Pure logic in `web/src/lib/` for
  testability.
- **Out of scope**: The popup UI, accept/revert actions, and the agent diff
  viewer tab. Those are Story 2 and Story 3.

## Dependencies

- Existing `FileWritten` event and event store replay.

## Acceptance

- `GET /api/sessions/{id}/edited-files` (or equivalent event filter) returns the
  list of files an agent has written in that session.
- `useEditedFiles` hook tracks the list live as events stream in.
- Pure aggregation logic is unit-tested in `web/src/lib/__tests__/`.
- `make check` passes.

## Verification

```text
make check
```
