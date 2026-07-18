# Story S-HIST-MIGRATE: Local History Migration (or Explicit Defer)

> **Status:** pending | **Difficulty:** small
> **Epic:** [agent-owned session history](../active-acp-agent-session-history-med.md).
> **Depends on:** epic migration Decision Needed; ideally after S-HIST-OPEN +
> S-HIST-FALLBACK so both stores' roles are clear.
> **Blocks:** none for MVP browse/open if migration is deferred.

## Goal

Resolve what happens to existing `conversations.json` rows and SQLite event
history once agent-owned list/load is the preferred path for capable agents —
either ship a concrete migration, or **explicitly defer** with STATUS /
known-issues language (no silent half-migration).

## Background / current behavior

- Durable metadata: `~/.local-agent/conversations.json` (`StoredSession` +
  `acpSessionId`).
- Transcripts: SQLite event store; UI sync via WS replay.
- Users already have real local history from pre-epic builds.

## Desired behavior (options — pick via Decision Needed)

| Option | Meaning |
|--------|---------|
| **A. Keep forever** | Local store remains fallback + archive; no import job. |
| **B. One-time import** | Map local threads into thin index / attempt agent association by `acpSessionId` where present; document orphans. |
| **C. Deprecate** | Stop writing new local transcripts for capable agents; read-only legacy view with sunset note. |

This story implements the **locked** option only. If product does not decide,
the deliverable is: epic + STATUS mark migration **deferred**, AC checklist
below left unchecked, and a known-issues bullet — still closes the
"forgotten migration" gap.

## Acceptance criteria

- [ ] Epic Q6 decision recorded as locked **or** explicitly deferred with
      owner/date.
- [ ] If A: document dual-read behavior (agent list + local) and tests that
      legacy rows still open via fallback.
- [ ] If B: migration runs once, is idempotent, fails loudly on corrupt
      state, leaves an audit/log of migrated vs orphaned ids.
- [ ] If C: legacy read-only UI + no new writes for capable harnesses;
      incapable harnesses unchanged (S-HIST-FALLBACK).
- [ ] If deferred: `docs/known-issues.md` + STATUS note; no partial schema
      rewrite.
- [ ] Lint/tests clean if code ships; docs-only defer needs STATUS update only.

## Out of scope

- Rewriting Blueprint event-sourcing § until architecture decisions (Q1)
  lock.
- Non-ACP provider history importers.

## Decision Needed

- Epic Q6 — **Migration** (keep / import / deprecate / defer).
- Related: Epic Q5 — **Delete / rename** (client-local vs agent) before any
  deprecate path removes UI affordances.
