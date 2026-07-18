# Story S-HIST-FALLBACK: Agents Without list / load

> **Status:** complete | **Difficulty:** med
> **Epic:** [agent-owned session history](../active-acp-agent-session-history-med.md).
> **Depends on:** S-HIST-PROBE; epic "agents without list/load" decision.
> **Independent of:** S-HIST-MIGRATE (migration is separate).

## Goal

When the selected harness does not advertise `session/list` and/or
`loadSession`, keep a **usable** conversation UX via the existing
daemon-owned path (`conversations.json` + event store) — without breaking
the agent-owned history flow for capable harnesses.

## Background / current behavior

- Today every harness effectively uses local conversations + events; agent
  list/load is reconcile-only for our own `acpSessionId`.
- After S-HIST-BROWSE/OPEN, capable agents shift toward agent-owned history;
  incapable agents must not get a dead-end empty history UI.

## Desired behavior

- Capability gate (from S-HIST-PROBE):
  - **No list:** history UI uses local `conversations.json` list (current
    behavior) for that harness; copy explains agent history unavailable.
  - **No load:** opening past local threads uses current restart/rebind
    semantics; do not call `session/load`. Foreign (other-editor) threads are
    unavailable — fail loudly if user somehow supplies a foreign id.
- Do not invent paste-session-id or one-shot-only modes unless the epic
  Decision Needed locks them — default story assumption is **keep local
  conversations.json behavior** until product says otherwise.
- Mixing: per-harness mode is fine (Codex agent-owned, other agent local).

## Acceptance criteria

- [x] Harness without list: ChatHistory retains local conversations; no empty
      "agent history" dead end.
- [x] Harness without load: existing load gate keeps local restart/rebind
      behavior and does not send `session/load`.
- [x] Harness with list+load: fallback notice is absent, preserving the future
      agent-history browse path.
- [x] User-visible explanation when live caps show list/load is unsupported.
- [x] Tests cover list/load capability combinations (`src/acp/providers.rs`).
- [x] Rust + frontend lint/tests/build clean.

## Out of scope

- Building a full agent-history browser for incapable agents.
- One-time import of foreign non-ACP transcripts.
- Deciding long-term deprecation of local store (S-HIST-MIGRATE).

## Decision Needed

- Epic Q3 — **locked 2026-07-18:** retain local `conversations.json` + events.
  Do not add paste-session-id or new-session-only modes.
