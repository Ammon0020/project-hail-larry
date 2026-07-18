# Story S-ACP-AUTODETECT: Agent Registry and Autodetection

> **Phase:** 3 | **Depends on:** S-CONFIG | **Go source:** `internal/acp/agent_registry.go`, `autodetect.go`

## Goal

Port registered-agent configuration and safe ACP capability probing separately
from live session handling.

## Acceptance Criteria

- [x] Registry persistence is compatible with existing configured agents
- [x] Probes use fixed executable/argument handling, timeouts, bounded output, and redacted logs
- [x] Autodetection does not execute user-controlled shell strings
- [x] Agent/model sorting and warning output match contract fixtures
- [x] Registry and probe tests pass without a live proprietary agent
