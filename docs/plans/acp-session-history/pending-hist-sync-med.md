# Story S-HIST-SYNC: Thin Multi-UI Active-Session Index

> **Status:** pending | **Difficulty:** med
> **Epic:** [agent-owned session history](../active-acp-agent-session-history-med.md).
> **Depends on:** S-HIST-OPEN (writes active selection); epic active-session
> shape decision.
> **Parallel-friendly with:** S-HIST-FALLBACK (after shape locked).

## Goal

Keep every paired device in sync on **which session is active** per workspace
(and enough metadata to render the sidebar) without making the daemon the
authoritative long-term chat archive. Authoritative history remains the agent;
the daemon owns a **thin index** + live broadcast.

## Background / current behavior

- Paired UIs sync via WebSocket hub + SQLite event replay (`?after=`).
- Session list/metadata: `conversations.json` + REST `/sessions`.
- Multi-device/single-user is already decided (`OpenItems.md`).

## Desired behavior

- Shared state (exact fields **Decision Needed**) at minimum enough to answer:
  for workspace W, which `agentId` / `acpSessionId` / title (cache) is selected.
- Updates broadcast to all paired clients when active session changes (open,
  switch, close/rebind).
- List/browse UI may show a **cached** last agent-list snapshot for snappy
  paint; refresh re-queries the agent when capable (S-HIST-BROWSE).
- Do **not** require importing full transcripts into SQLite for sync correctness
  unless Q1 locks a daemon transcript cache — if locked, document cache
  invalidation vs agent load.

## Acceptance criteria

- [ ] Documented + implemented thin index shape matching the locked decision
      (or AC blocked until Decision Needed resolves — do not invent fields in
      code without a decision).
- [ ] Switching active session on device A updates device B without full page
      reload (WS or existing sync path).
- [ ] Index survives daemon restart (durable file or embedded store — choose
      one; prefer extending `conversations.json` / adjacent thin file over a
      second SQLite schema unless justified).
- [ ] Auth: only paired devices; no unauthenticated session enumeration.
- [ ] Tests: concurrent clients see same active mapping; restart restores index.
- [ ] Lint/tests clean for touched packages.

## Out of scope

- Full agent history browser (S-HIST-BROWSE) beyond consuming its selection.
- Offline-first full transcript browsing without an agent.
- Multi-user / per-device private sessions (explicitly out; single-user).

## Decision Needed

- Epic Q2 — **Active-session sync** shape (`workspaceId → {…}` minimal map?).
- Epic Q1 — whether index may embed/cache transcript snippets or only metadata.
