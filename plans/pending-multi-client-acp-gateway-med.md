# Epic: Multi-Client ACP Gateway

> **Status:** Pending. **Created:** 2026-07-26.
> **Related:** `docs/reference/acp/responsibilities.md`, `docs/specs/backend-spec.md`

## Goal

Enable multiple clients (e.g., Windsurf/Devin and our web/mobile IDE) to view, control, and interact with the same active ACP session concurrently through our host daemon.

## Protocol Position & Architecture

- **ACP is 1:1 at transport level:** The ACP specification mandates a single JSON-RPC connection between one host client and one agent process. Direct multi-client attachment to a raw agent process is not supported by ACP and would cause protocol collision and state corruption.
- **Daemon Proxy Architecture:** Our host daemon operates as the authoritative, single ACP Client to the agent process while acting as a multi-client gateway hub to external frontends (Web UI, Mobile, Windsurf, Devin).
- **Fan-Out & Event Replay:** ACP event updates (`session/update`, thoughts, tool status, plan changes) received by the daemon are broadcast to all attached clients via WebSocket. Late-joining clients receive event replay from the SQLite WAL store.
- **Unified Permission Gate:** `session/request_permission` calls from the agent are broadcast to all connected devices. A decision made by any client resolves the request for the session and updates all attached UIs.

## Story Index

| ID | Story | Size | Depends on | Acceptance |
|---|---|---:|---|---|
| S-GATEWAY-FANOUT | [Multi-client event fan-out & session replay](multi-client-acp-gateway/pending-gateway-fanout-med.md) | med | — | Broadcast ACP updates to N subscribers & replay WAL on attach |
| S-GATEWAY-PERM | [Synchronized permission resolution & conflict handling](multi-client-acp-gateway/pending-gateway-perm-small.md) | small | FANOUT | First-wins permission decisions with live state sync across clients |
| S-GATEWAY-ADAPTER | [External IDE adapter & session attach API](multi-client-acp-gateway/pending-gateway-adapter-med.md) | med | FANOUT | External clients (Windsurf/Devin) attach & prompt active daemon sessions |

## Boundaries

**In scope:** Daemon WS multi-subscriber fan-out, event replay on client attach, synchronized permission handling, IDE attachment REST/WS protocol adapter.

**Out of scope:** Modifying the upstream ACP specification, running multiple agent processes for a single session, or bypassing the daemon's filesystem/permission controls.

## Cross-cutting risks

- **Permission race conditions:** Simultaneous approval/rejection from two clients must resolve idempotently (first-valid-decision wins).
- **Stale client context:** Reconnecting or late-attaching external IDE clients must complete event replay before accepting new user input.
- **Concurrent edits:** Out-of-band edits from external IDEs and web editor must leverage existing revision tracking and three-way merge.

## Verification Bar

- Integration tests simulating multiple concurrent WS subscribers receiving streaming agent events.
- Unit/contract tests verifying permission decision race resolution across multiple clients.
- End-to-end verification attaching a secondary client (Windsurf/Devin mock) to an active session.
