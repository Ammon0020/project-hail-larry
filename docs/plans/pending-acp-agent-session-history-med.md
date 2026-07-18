# Epic: Agent-Owned ACP Session History

> **Status:** Research captured — **needs to be fleshed out** (no stories yet).
> **Owner:** —. **Created:** 2026-07-18.
> **Related:** `docs/reference/acp/responsibilities.md`, `docs/plans/acp-spec-compliance.md` (§4.4 list reconcile, §4.6 fork/resume/close), Blueprint §9–12.

## Goal

Stop treating our daemon as the long-term chat archive. Prefer the ACP model
used by clients like Zed / Devin Desktop: the **agent** persists conversations;
we **discover** them with `session/list`, **open** them with `session/load`
(or `session/resume`), and keep only a thin index so every paired UI stays in
sync (which session is active, which workspace/`cwd` it belongs to).

Users should see and resume chats started in *any* ACP-compatible editor that
talked to the same agent, filtered by folder/workspace when useful.

## Status of this epic

This document is a **findings dump + direction**, not an executable plan.

**Still needed before implementation:**

- Story breakdown (browse, open, multi-UI sync, fallbacks, migration)
- Explicit decisions on the open questions below
- Capability matrix per harness (which agents advertise list / load / resume)
- UX sketch for history when the agent is the source of truth
- Migration story for existing `conversations.json` + SQLite event history

Do **not** treat the Architecture Direction section as locked — it is the
working hypothesis from research.

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

So yes: we can see which folder a chat is associated with. Filtering
`session/list` with `cwd` scopes history to a workspace root. Omitting `cwd`
returns a broader list (still each entry carries its own `cwd`).

List supports cursor pagination (`cursor` / `nextCursor`). Content is
intentionally **not** in the list response — open via load.

### What we do today

| Concern | Current behavior |
|---------|------------------|
| UI session list | Our `conversations.json` (daemon-owned metadata) |
| Chat transcript | SQLite event store; UIs sync via WebSocket + event replay |
| Agent `session/list` | Used only to **reconcile** a known `acpSessionId` before load (`resolveACPSession`) — not exposed as history UI |
| Agent `session/load` | Used on restart/resume for sessions **we already created** |
| Foreign sessions (Zed, CLI, other editors) | Not discoverable or openable in our UI |

We already have transport wrappers for `ListSessions` / `LoadSession` and
persist `ACPSessionID` — the gap is product/architecture: agent registry as
source of truth vs client event archive.

### Folder ↔ chat association

- Workspace path in our app ≈ ACP `cwd` on `session/new` / `session/load` /
  `session/list` filter.
- Matching depends on **absolute path equality** (and agent behavior). Same
  project opened via different path spellings (symlink, `..`, case) may not
  match — needs a flesh-out decision (canonicalize? show unmatched under
  “other folders”?).
- Multi-root: `additionalDirectories` on list/load when advertised.

---

## Architecture direction (working hypothesis)

**Prefer agent-owned history; daemon owns thin sync state only.**

```text
  [Agent session store]  --session/list-->  history browser (by cwd / all)
           |
           +--session/load-->  replay updates into live UI(s)
           |
  [Daemon thin index]  --WS-->  all paired devices
       activeSessionId, agentId, cwd/workspaceId, title cache?
```

Implications:

- We may **not** need a durable chat event archive for ACP transcripts if every
  open is a load/resume against the agent and UIs are thin.
- We **do** still need something shared across paired devices: at least which
  `acpSessionId` is selected per workspace, and enough metadata to render the
  sidebar without re-probing the agent on every paint (cache of last list
  result is fine; authoritative list remains the agent).
- Live turns still flow through the daemon (ACP client layer) so permissions,
  FS, and shell stay client-owned — unchanged Blueprint principle.

This is a deliberate pivot from “import foreign sessions into SQLite and keep
storing everything ourselves.” Import-into-events remains a **fallback** for
agents that lack list/load, or if multi-device replay economics demand a
daemon-side cache — decide in flesh-out.

---

## Open questions (must resolve when fleshing out)

1. **Zero durable transcript vs cache** — Is replay-from-agent on every device
   open acceptable, or do we cache the last loaded transcript on the daemon for
   fast multi-UI sync / offline sidebar previews?
2. **Active-session sync** — Minimal shared state shape: map
   `workspaceId → { agentId, acpSessionId, title }` only?
3. **Agents without list/load** — Fallback UX (our old store? one-shot new
   session only? paste session id?).
4. **`load` vs `resume`** — First open = load (full replay); reconnect with
   title already known = resume?
5. **Delete / rename** — Client-local only vs agent `session/delete` /
   `session_info_update`?
6. **Migration** — Existing `conversations.json` + SQLite events: keep forever,
   one-time import, or deprecate?
7. **Path canonicalization** — How we map registered workspaces to `cwd`
   filters when other editors used a different absolute path.
8. **Probe cost** — Listing requires an initialized agent process; cold start
   strategy for history UI.

---

## Current code touchpoints (for later stories)

- `internal/acp/transport.go` — `ListSessions`, `LoadSession`
- `internal/acp/acp.go` — `resolveACPSession` (reconcile-only use of list)
- `internal/acp/store.go` — `conversations.json`
- `internal/server/api.go` — `handleListSessions` (our conversations, not agent)
- Frontend chat history / tabs — driven by daemon session list + events

---

## Context7 / ACP resources

Use Context7 when implementing or updating this epic. Prefer these library IDs:

| Context7 library ID | Use for |
|---------------------|---------|
| `/websites/agentclientprotocol` | Canonical protocol docs site (session setup, list RFD, capabilities) |
| `/agentclientprotocol/agent-client-protocol` | Spec / schema (list, load, SessionInfo) |
| `/llmstxt/agentclientprotocol_llms-full_txt` | Broad llms-full dump when hunting edge cases |
| `/agentclientprotocol/rust-sdk` | Rust port session APIs |
| Resolve `coder/acp-go-sdk` via Context7 when touching Go transport | Go client method names / types |

**Primary pages (also fetchable without Context7):**

- [Session setup — new / load / resume](https://agentclientprotocol.com/protocol/v1/session-setup)  
  Explicitly: load enables “sharing sessions between different Client instances”;
  load replays history via `session/update`; resume does not replay.
- [Session list RFD](https://agentclientprotocol.com/rfds/session-list)  
  Discovery, `cwd` filter, pagination, SessionInfo fields; list ≠ load.
- [Protocol overview](https://agentclientprotocol.com/protocol/overview)
- Local notes: `docs/reference/acp/spec.md`, `docs/reference/acp/responsibilities.md`

**Suggested Context7 queries:**

- `session/list SessionInfo cwd title loadSession sessionCapabilities.list`
- `session/load replay session/update vs session/resume`
- `ListSessions LoadSession InitializeResponse AgentCapabilities` (SDK-specific)

---

## Out of scope (for now)

- Story-level estimates, acceptance criteria, or sprint sequencing
- Changing Blueprint event-sourcing language until decisions above land
- Provider-specific (non-ACP) history importers

## Next step

Flesh out this epic: lock architecture answers to the open questions, then
split into stories (browse-by-cwd, open/load, thin multi-UI index, agent
capability fallbacks, migration). Until then, keep using the current
conversations + event store.
