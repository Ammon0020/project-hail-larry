# Agent process watchdog + heartbeat

> **Status:** done | **Difficulty:** hard | **Urgency:** high
> **Source:** chat reliability audit

## Problem

If an agent process hangs without exiting (e.g. deadlocked, network-stuck,
infinite loop), the session stays in `Running` state indefinitely. The
terminal watcher (`src/acp/core/lifecycle/mod.rs:134-160`) only fires when
the process actually exits. There is no heartbeat or health check.

## Goal

Add a watchdog that detects hung agent processes and surfaces a clear
error to the user, transitioning the session to `Failed`.

## Behavior

1. After a configurable idle period (default 120s) with no ACP events
   from the agent, the watchdog fires.
2. The daemon sends a `Ping` (or equivalent ACP keepalive) to the agent.
3. If the agent responds within a grace period (default 30s), the watchdog
   resets.
4. If the agent does not respond, the session transitions to `Failed`
   with an `AgentExited` event: "Agent unresponsive (no heartbeat for
   {N}s)".
5. The frontend shows this as a chat error banner with a "Restart
   session" action.

## Dependencies

- ACP keepalive/ping mechanism (or process-level liveness check)
- Configurable timeout in `config.toml`

## Acceptance

- [x] Hung agent detected within idle period
- [x] Session transitions to `Failed` with descriptive event
- [x] Frontend shows error banner (existing `AgentExited` handling in ChatPanel.tsx)
- [x] Configurable timeout via `config.toml` (`agentIdleTimeoutSeconds`)
- [x] Tests: simulated hang triggers watchdog
- [x] `make check` passes
