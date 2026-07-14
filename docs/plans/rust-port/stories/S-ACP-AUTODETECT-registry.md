# Story S-ACP-AUTODETECT: Agent Registry and Autodetection

> **Phase:** 3 | **Depends on:** S-CONFIG | **Go source:** `internal/acp/agent_registry.go`, `autodetect.go`

## Goal

Port registered-agent configuration and safe ACP capability probing separately
from live session handling.

## Acceptance Criteria

- [ ] Registry persistence is compatible with existing configured agents
- [ ] Probes use fixed executable/argument handling, timeouts, bounded output, and redacted logs
- [ ] Autodetection does not execute user-controlled shell strings
- [ ] Agent/model sorting and warning output match contract fixtures
- [ ] Registry and probe tests pass without a live proprietary agent
