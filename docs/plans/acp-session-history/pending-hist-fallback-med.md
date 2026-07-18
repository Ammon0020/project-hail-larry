# Story S-HIST-FALLBACK: Agents Without list / load

> **Status:** pending | **Difficulty:** med
> **Epic:** [agent-owned session history](../pending-acp-agent-session-history-med.md).
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

- [ ] Harness without list: ChatHistory still shows local conversations;
      no empty "agent history" dead end.
- [ ] Harness without load: local resume/rebind still works; no spurious
      LoadSession RPC.
- [ ] Harness with list+load: fallback path does not override agent browse
      (regression test).
- [ ] User-visible explanation when agent history is unsupported.
- [ ] Tests for both capability combinations (matrix subset).
- [ ] Lint/tests clean for touched packages.

## Out of scope

- Building a full agent-history browser for incapable agents.
- One-time import of foreign non-ACP transcripts.
- Deciding long-term deprecation of local store (S-HIST-MIGRATE).

## Decision Needed

- Epic Q3 — confirm fallback = keep local `conversations.json` (+ events),
  vs alternatives (new-session-only, paste id). **Do not implement
  alternatives until locked.**
