# Task: Kill ACP agent process descendants on shutdown

> **Status:** pending | **Difficulty:** med | **Urgency:** med
> **Origin:** S-ACP-CORE audit (2026-07-18). Epic: rust-port.

## Problem

`docs/plans/rust-port/active-S-ACP-CORE-session-transport-med.md` AC#3 is
partially implemented. Terminal commands get proper process-group killing via
`src/shell/mod.rs:567-650` (`setpgid` + `killpg`), but the ACP agent process
itself (`src/acp/core.rs:1189-1196`) uses `kill_on_drop(true)` + `child.kill()`
which kills ONLY the direct child, NOT process descendants. If an agent spawns
subprocesses, those survive daemon shutdown.

## Scope

- Apply process-group setup (`setpgid` or equivalent) to the ACP agent
  `Command` in `run_actor_inner` (`src/acp/core.rs`)
- On cancellation/shutdown, kill the entire process group, not just the child
- Add a test: agent spawns a subprocess, daemon shuts down, both are reaped
- Mirror the existing `src/shell/mod.rs` process-group pattern

## Acceptance criteria

- [ ] ACP agent process and all descendants are killed on cancellation/shutdown
- [ ] Test verifies descendant processes are reaped
- [ ] No regression in existing ACP lifecycle tests

## Out of scope

- Cross-platform process-group handling on Windows (may need `taskkill /T`)
