# Story S-HIST-BROWSE: Browse Agent Sessions by cwd / Workspace

> **Status:** pending | **Difficulty:** med
> **Epic:** [agent-owned session history](../pending-acp-agent-session-history-med.md).
> **Depends on:** S-HIST-PROBE (cap gate); epic path-canonicalization decision
> for correct filtering.
> **Blocks:** S-HIST-OPEN (needs selectable list).

## Goal

Let the user browse sessions the **agent** knows about — filtered by the
active workspace root (`cwd`) when useful — including threads started in other
ACP clients (Zed, CLI, etc.), not only rows in our `conversations.json`.

## Background / current behavior

- UI history (`web/src/components/ChatHistory.tsx`) lists daemon sessions from
  `GET /sessions` → `ConversationStore` / live registry.
- Agent `session/list` is used only inside `resolve_acp_session` for reconcile.
- ACP `SessionInfo` carries `sessionId`, `cwd`, optional `title` /
  `updatedAt` / `additionalDirectories`; list supports cursor pagination.

## Desired behavior

### Backend

- Auth-gated API to list agent sessions for a harness + optional
  `workspaceId`/`cwd` filter (pagination via agent `cursor` / `nextCursor`).
- Map registered workspace → absolute `cwd` for the filter (canonicalization
  per epic Decision Needed).
- When list capability is absent, return a clear capability error (or empty +
  flag) so UI can fall through to S-HIST-FALLBACK — do not fake agent rows
  from SQLite.

### Frontend

- History UI can show an **agent-sourced** list (title, updatedAt, cwd) scoped
  to the current workspace, with an explicit way to widen to "all folders"
  if product locks that (Decision Needed: unmatched paths).
- Loading / error / "agent cold" states fail loudly with actionable copy.
- Selecting a row hands off to S-HIST-OPEN (this story may stop at selection
  callback + stub if OPEN is sequenced after).

## Acceptance criteria

- [ ] Agent `session/list` results reachable from authenticated API with
      cwd/workspace filter.
- [ ] Pagination works when the agent returns `nextCursor`.
- [ ] Foreign sessions (same agent, other client) appear when the agent lists
      them (mockagent or documented real-agent check).
- [ ] No list capability → structured error/flag; UI does not pretend agent
      history exists.
- [ ] Path filter behavior matches locked canonicalization decision (or AC
      deferred with Decision Needed callout if still open).
- [ ] Tests: list happy path, empty, no-cap, traversal-safe cwd mapping.
- [ ] Lint/tests clean for touched packages.

## Out of scope

- Full transcript render / replay (S-HIST-OPEN).
- Thin multi-device active-session index writes (S-HIST-SYNC) — browse may
  read cached list metadata only.
- Rename/delete of agent sessions.
- Migrating old local history (S-HIST-MIGRATE).

## Decision Needed

- Epic Q7 — **Path canonicalization** (symlink / case / alternate abs paths).
- Epic Q8 — **Probe cost** (cold agent for history-only browse).
- Whether "other folders" / unmatched cwd rows are shown in v1.
