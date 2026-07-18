# Story S-HIST-OPEN: Open / Load a Past Agent Session

> **Status:** pending | **Difficulty:** med
> **Epic:** [agent-owned session history](../active-acp-agent-session-history-med.md).
> **Depends on:** S-HIST-PROBE, S-HIST-BROWSE; epic load-vs-resume + transcript
> cache decisions for complete AC.
> **Blocks:** S-HIST-SYNC (active session after open); informs S-HIST-FALLBACK.

## Goal

When the user picks an agent-listed session, open it in our UI via ACP
`session/load` (replay history as `session/update`s) or `session/resume` when
that is the locked policy — then continue with `session/prompt` under the same
client-owned FS/shell/permissions model.

## Background / current behavior

- `resolve_acp_session` loads only sessions **we** already created and
  persisted as `acpSessionId` in `conversations.json`.
- Live turns already stream via EventBus → Hub; chat UI renders from events.
- Spec: `session/load` replays history; `session/resume` restores context
  **without** replaying (see agentclientprotocol session-setup).

## Desired behavior

- From an agent `sessionId` (+ harness + cwd/workspace), call load (or resume
  per Decision Needed) through the ACP client layer — never from the browser
  directly to the agent.
- Replayed `session/update`s become UI-visible chat (via existing event /
  live-fanout path, or a documented temporary bridge — implementor chooses
  the thinnest path that keeps multi-device consistent with S-HIST-SYNC).
- Persist enough thin index fields after open (`acpSessionId`, harness,
  workspace, title cache) so restart/reconcile still works.
- Fail loudly if load capability missing, session gone, or cwd mismatch —
  clear user-visible error; no silent `session/new` unless product locks that
  fallback for this path (prefer explicit).

## Acceptance criteria

- [ ] User can open a listed agent session and see prior turns (load path) or
      documented resume-without-replay UX if that decision is chosen.
- [ ] Permissions / FS / shell remain client-owned for subsequent prompts.
- [ ] Opened session appears as the active conversation on the opening device;
      thin index updated for other devices once S-HIST-SYNC lands (until then,
      document interim single-device behavior).
- [ ] Missing session / no loadSession → explicit error (tested).
- [ ] Mockagent (or fixture) covers load replay into UI/event stream.
- [ ] Lint/tests clean for touched packages.

## Out of scope

- Implementing the full multi-UI index protocol (S-HIST-SYNC).
- Agents without load (S-HIST-FALLBACK).
- Delete/rename on the agent.
- Changing Blueprint event-sourcing language wholesale.

## Decision Needed

- Epic Q1 — **Zero durable transcript vs cache** (replay every open vs daemon
  cache for multi-UI / offline preview).
- Epic Q4 — **`load` vs `resume`** (when to use each).
