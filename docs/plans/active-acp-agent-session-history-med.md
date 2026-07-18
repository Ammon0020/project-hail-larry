# Epic: Agent-Owned ACP Session History

> **Status:** In progress — probe and fallback complete; browse/open remain
> decision-blocked.
> **Owner:** —. **Created:** 2026-07-18. **Updated:** 2026-07-18.
> **Related:** `docs/reference/acp/responsibilities.md`,
> `docs/plans/acp-spec-compliance.md` (§4.4 list reconcile, §4.6 fork/resume/close),
> Blueprint §9–12. Stories: `docs/plans/acp-session-history/`.

## Goal

Stop treating our daemon as the long-term chat archive. Prefer the ACP model
used by clients like Zed / Devin Desktop: the **agent** persists conversations;
we **discover** them with `session/list`, **open** them with `session/load`
(or `session/resume`), and keep only a thin index so every paired UI stays in
sync (which session is active, which workspace/`cwd` it belongs to).

Users should see and resume chats started in *any* ACP-compatible editor that
talked to the same agent, filtered by folder/workspace when useful.

## Status of this epic

Probe and fallback are complete. Browse, open, sync, and migration remain
blocked on their listed decisions (do not invent answers in code).

Architecture Direction remains a **working hypothesis** until Q1–Q2 lock.

---

## Story Index

| ID | Story | Size | Depends on | Acceptance |
|----|-------|------|------------|------------|
| S-HIST-PROBE | [Capability probe + harness matrix](acp-session-history/complete-hist-probe-small.md) | small | — | ✅ complete (live-only; Q8 open) |
| S-HIST-BROWSE | [Browse agent sessions by cwd](acp-session-history/pending-hist-browse-med.md) | med | PROBE; Q7/Q8 | story AC |
| S-HIST-OPEN | [Open/load past session into UI](acp-session-history/pending-hist-open-med.md) | med | PROBE, BROWSE; Q1/Q4 | story AC |
| S-HIST-SYNC | [Thin multi-UI active-session index](acp-session-history/pending-hist-sync-med.md) | med | OPEN; Q2 (Q1) | story AC |
| S-HIST-FALLBACK | [Fallback without list/load](acp-session-history/complete-hist-fallback-med.md) | med | PROBE; Q3 | ✅ complete |
| S-HIST-MIGRATE | [Local history migrate or defer](acp-session-history/pending-hist-migrate-small.md) | small | Q6 (Q5) | story AC |

**Next sequence:** lock Q7/Q8, then BROWSE → OPEN → SYNC; MIGRATE last or
docs-only defer after Q6.

---

## Decision Needed (blocking implementation)

Do **not** invent answers. Record locks here when product decides.

| # | Topic | Blocks | Status |
|---|-------|--------|--------|
| Q1 | **Zero durable transcript vs cache** — replay-from-agent on every device open, or daemon cache of last loaded transcript for multi-UI / offline sidebar? | OPEN, SYNC | open |
| Q2 | **Active-session sync shape** — e.g. `workspaceId → { agentId, acpSessionId, title }` only? | SYNC | open |
| Q3 | **Agents without list/load** — retain local `conversations.json` + events; no paste-id or new-session-only mode. | FALLBACK | **locked 2026-07-18** |
| Q4 | **`load` vs `resume`** — first open = load (full replay); reconnect with known title = resume? | OPEN | open |
| Q5 | **Delete / rename** — client-local only vs agent `session/delete` / `session_info_update`? | MIGRATE (deprecate), later UX | open |
| Q6 | **Migration** — keep forever / one-time import / deprecate / **explicit defer**? | MIGRATE | open |
| Q7 | **Path canonicalization** — map workspace ↔ `cwd` when other editors used different abs paths (symlink, case, `..`)? | BROWSE | open |
| Q8 | **Probe cost** — may history UI cold-start an agent process to list, or only when warm? | PROBE, BROWSE | open |

Also UX (non-blocking for PROBE): history UI sketch when agent is source of
truth — draft during BROWSE/OPEN, not a separate gate.

---

## Findings

### What other ACP clients do

Editors such as Zed (and Devin Desktop as an ACP host) can open past threads
from external agents because:

1. The **agent** stores session state (its own DB / files).
2. The client calls **`session/list`** to enumerate sessions (often filtered by
   project directory).
3. The user picks one; the client calls **`session/load`**.
4. The agent **replays** the transcript via `session/update` notifications,
   then the client continues with `session/prompt`.

Cross-editor continuity is a property of **agent-side persistence + list/load**,
not of a shared editor-side chat database.

Evidence in the wild: Zed’s thread history / import path for external ACP
agents; issues when `session/list` returns sessions but the client UI does not
surface them (client gap, not protocol gap).

### ACP protocol facts (verified)

| Method / capability | Role |
|---------------------|------|
| `agentCapabilities.loadSession` | Gate for `session/load` |
| `sessionCapabilities.list` | Gate for `session/list` |
| `session/list` | Discovery only — metadata, not full content |
| `session/load` | Restore + **replay** full history as `session/update`s |
| `sessionCapabilities.resume` + `session/resume` | Restore context **without** replaying history |
| `sessionCapabilities.close` / delete | Lifecycle cleanup (optional) |

**`SessionInfo` (from `session/list`) includes folder association:**

- `sessionId` (required)
- `cwd` (required) — **working directory / folder for the session**
- `title` (optional)
- `updatedAt` (optional, ISO 8601)
- `_meta` (optional, agent-specific)
- `additionalDirectories` (when multi-root is in play)

Filtering `session/list` with `cwd` scopes history to a workspace root.
Omitting `cwd` returns a broader list (each entry still carries its own `cwd`).
List supports cursor pagination. Content is **not** in the list response.

### What we do today

| Concern | Current behavior |
|---------|------------------|
| UI session list | `conversations.json` (daemon-owned metadata) |
| Chat transcript | SQLite event store; UIs sync via WebSocket + event replay |
| Agent `session/list` | Reconcile known `acpSessionId` before load (`resolve_acp_session`) — not history UI |
| Agent `session/load` | Restart/resume for sessions **we** already created |
| Foreign sessions (Zed, CLI, other editors) | Not discoverable or openable in our UI |

Transport wrappers and `acpSessionId` persistence exist (Rust `src/acp/core.rs`,
`store.rs`). Gap is product/architecture: agent registry as source of truth vs
client event archive.

### Folder ↔ chat association

- Workspace path ≈ ACP `cwd` on `session/new` / `session/load` / `session/list`.
- Matching depends on absolute path equality (and agent behavior). Alternate
  spellings may not match — see **Q7**.
- Multi-root: `additionalDirectories` on list/load when advertised.

---

## Architecture direction (working hypothesis — not locked)

**Prefer agent-owned history; daemon owns thin sync state only.**

```text
  [Agent session store]  --session/list-->  history browser (by cwd / all)
           |
           +--session/load-->  replay updates into live UI(s)
           |
  [Daemon thin index]  --WS-->  all paired devices
       activeSessionId, agentId, cwd/workspaceId, title cache?
```

Implications (pending Q1–Q2):

- May not need a durable chat event archive for ACP transcripts if every open
  is load/resume against the agent and UIs are thin.
- Still need shared state across paired devices: at least which `acpSessionId`
  is selected per workspace (+ metadata cache).
- Live turns still flow through the daemon ACP client — Blueprint unchanged.
- Import-into-events remains a **fallback** for agents lacking list/load, or if
  multi-device economics demand a daemon cache — decide via Q1/Q3.

---

## Current code touchpoints

- `src/acp/core.rs` — `ListSessionsRequest`, load path, `resolve_acp_session`
- `src/acp/store.rs` — `conversations.json` / `StoredSession`
- `src/api/` — REST session list (daemon conversations, not agent)
- `web/src/components/ChatHistory.tsx` — UI driven by daemon sessions + events
- `src/acp/agent_registry.rs`, `autodetect.rs` — harness matrix inputs

---

## Context7 / ACP resources

| Context7 library ID | Use for |
|---------------------|---------|
| `/websites/agentclientprotocol` | Protocol docs (session setup, list RFD, capabilities) |
| `/agentclientprotocol/agent-client-protocol` | Spec / schema (list, load, SessionInfo) |
| `/llmstxt/agentclientprotocol_llms-full_txt` | Edge-case hunting |
| `/agentclientprotocol/rust-sdk` | Rust session APIs |

**Primary pages:**

- [Session setup — new / load / resume](https://agentclientprotocol.com/protocol/v1/session-setup)
- [Session list RFD](https://agentclientprotocol.com/rfds/session-list)
- Local: `docs/reference/acp/spec.md`, `docs/reference/acp/responsibilities.md`

---

## Out of scope (epic-wide)

- Implementing stories before Decision Needed items that each story marks
- Changing Blueprint event-sourcing language until Q1 locks
- Provider-specific (non-ACP) history importers
- Multi-user private session histories

## Next step

1. Lock Q7/Q8 for BROWSE (and Q1/Q4 before OPEN).
2. Implement BROWSE → OPEN → SYNC.
3. Resolve or explicitly defer migration per Q6.
