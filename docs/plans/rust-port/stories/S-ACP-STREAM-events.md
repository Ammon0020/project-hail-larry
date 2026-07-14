# Story S-ACP-STREAM: ACP Updates to Ordered App Events

> **Phase:** 3 | **Depends on:** S-ACP-CORE, S-CONTRACT | **Go source:** `internal/acp/messages.go`, `internal/acp/transport.go`

## Goal

Translate ACP notifications and tool progress into exhaustive typed internal
events, then serialize them through the stable frontend wire adapter.

## Acceptance Criteria

- [ ] Every supported ACP update variant has an explicit translation path or a visible compatibility error
- [ ] Events are persisted before publication and retain durable IDs
- [ ] Output, thoughts, plans, tool lifecycle, stop reason, and attachment shapes match Go fixtures
- [ ] Unknown agent data is logged with redaction and never silently discarded
- [ ] Contract and streaming integration tests pass
