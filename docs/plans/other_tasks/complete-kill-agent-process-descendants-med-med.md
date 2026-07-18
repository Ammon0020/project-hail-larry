# Task: Kill ACP agent process descendants on shutdown

> **Status:** complete | **Difficulty:** med | **Urgency:** med
> **Origin:** S-ACP-CORE audit (2026-07-18). Epic: rust-port.

## Problem

`docs/plans/rust-port/active-S-ACP-CORE-session-transport-med.md` AC#3 was
partially implemented. Terminal commands got process-group killing via shell,
but the ACP agent process itself used `kill_on_drop(true)` + `child.kill()`
which kills ONLY the direct child, NOT process descendants.

## Resolution (2026-07-18)

- Extracted shared `src/procutil/` (`configure_process_group`,
  `ProcessGroupCleanup`, `kill_process_group`); shell now uses it.
- ACP `run_actor_inner` builds via `std::process::Command`, applies process
  group, converts to `async_process::Command`; shutdown kills the group then
  reaps; drop guard covers early exits.
- Windows remains best-effort (child only / `CREATE_NEW_PROCESS_GROUP`); Job
  Object tree kill still out of scope.
- Tests: `procutil` drop-guard grandchild reap; ACP
  `agent_process_group_kill_reaps_descendant`.

## Acceptance criteria

- [x] ACP agent process and all descendants are killed on cancellation/shutdown
- [x] Test verifies descendant processes are reaped
- [x] No regression in existing ACP lifecycle tests

## Out of scope

- Cross-platform process-group handling on Windows (may need `taskkill /T`)
