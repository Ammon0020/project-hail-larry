# Story S-ACP-CORE: ACP Sessions and Transport Handlers

> **Phase:** 3 | **Depends on:** S-ACP-SPIKE, S-EVENTS, S-FILES, S-SHELL, S-PERMISSIONS | **Go source:** `internal/acp/acp.go`, `internal/acp/transport.go`, `internal/acp/terminal.go`
> **Status:** complete (2026-07-18)

## Goal

Implement per-session ACP process lifecycle and the SDK request handlers without
mixing event translation, conversation persistence, providers, or autodetect.

## Design

Each session has an `Arc` state object, cancellation token, process-tree handle,
and owned tasks. The session map is locked only for lookup/update; RPC and I/O
run after taking an `Arc` clone. Transport handlers delegate to typed files,
shell, and permissions services.

## Acceptance Criteria

- [x] Create/load/list/close and cancellation work with the verified SDK API
- [x] File and shell handlers preserve workspace and permission constraints
- [x] All session tasks and process descendants stop on cancellation/shutdown
- [x] No mutex/RwLock guard is held across `.await`
- [x] Mock-agent lifecycle and handler integration tests pass

## Notes

- Process-group kill for the agent binary (not only terminals) lives in
  `src/procutil/` and is wired from `run_actor_inner` (`src/acp/core.rs`).
  Windows remains child-only (Job Object deferred). See
  `docs/plans/other_tasks/complete-kill-agent-process-descendants-med-med.md`.
