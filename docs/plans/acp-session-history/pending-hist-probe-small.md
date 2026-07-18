# Story S-HIST-PROBE: Capability Probe + Harness Matrix

> **Status:** complete | **Difficulty:** small
> **Epic:** [agent-owned session history](../pending-acp-agent-session-history-med.md).
> **Depends on:** none (research-first; unblocks browse/open/fallback).
> **Blocks:** S-HIST-BROWSE, S-HIST-OPEN, S-HIST-FALLBACK (capability gates).

## Goal

Document which registered harnesses advertise `sessionCapabilities.list`,
`agentCapabilities.loadSession`, and `sessionCapabilities.resume`, and how we
probe them at runtime — so browse/open/fallback stories can branch on real
caps instead of guessing.

## Background / current behavior

- `src/acp/core.rs` — `resolve_acp_session` already reads `load_session` and
  optionally calls `ListSessionsRequest` (cwd-scoped) to reconcile a known
  `acp_session_id` before `LoadSession`. List is **not** exposed as history UI.
- `src/acp/agent_registry.rs` / `autodetect.rs` — harness registry + probes;
  no durable per-agent capability matrix for list/load/resume.
- Initialize response is the source of truth per live process; cold agents
  have no caps until started.

## Desired behavior

- Maintain a **checked harness matrix** (doc artifact under this epic folder or
  referenced from the epic) covering at least: Claude Code ACP, Codex ACP,
  Gemini CLI ACP, Cursor CLI ACP, OpenCode (and any other registry entries).
- Columns: `list`, `loadSession`, `resume`, `close`/`delete` (if known), notes
  (version / last verified date), probe method.
- Runtime: after `initialize`, surface negotiated caps to API/UI consumers in a
  stable shape (extend existing session/provider caps path or add a thin
  `GET` — exact route left to implementor; must be auth-gated).
- Cold-start policy for probing is **Decision Needed** on the epic
  (`Probe cost`) — this story documents options and implements whatever is
  locked; if still open, ship matrix + live-only cap readout and leave cold
  start as follow-on AC unchecked.

## Acceptance criteria

- [x] Written harness matrix checked in (markdown table) with verified or
      explicitly "unknown / not probed" cells — no invented ✅.
      → `docs/plans/acp-session-history/harness-session-history-matrix.md`
- [x] Live `initialize` caps for list / loadSession / resume readable by the
      client (test or contract fixture).
      → `GET /api/sessions/{id}/capabilities` + `SessionHistoryCapabilities`
- [x] Agents lacking list and/or load are identifiable so S-HIST-FALLBACK can
      gate UX. (`available && !canListSessions && !canLoadSession`)
- [x] Unit/integration coverage for cap projection (mock agent with/without
      list+load). (`src/acp/providers.rs` tests)
- [x] `cargo test -q` (scoped) + lint clean for touched Rust; no UI redesign.
- [ ] ~~Cold-start probe when agent is not warm~~ — **deferred** (epic Q8
      open). Live-only readout ships; dormant → `available: false`.

## Out of scope

- History browser UI (S-HIST-BROWSE).
- Opening foreign sessions (S-HIST-OPEN).
- Changing `conversations.json` schema.
- Provider-specific (non-ACP) history APIs.

## Decision Needed (blocking full cold-start AC)

- Epic Q8 — **Probe cost:** may we spawn/initialize an agent solely to list
  history, or only probe when a worker is already warm?
  **Shipped:** live-only. Follow-on when Q8 locks.
