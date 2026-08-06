# Frontend response timeout for stuck agentRunning

> **Status:** done | **Difficulty:** medium | **Urgency:** medium
> **Source:** chat reliability audit

## Problem

If `agentRunning` stays true with no events arriving, there is no
automatic recovery on the frontend. The `useSendingState` hook
(`web/src/hooks/useSendingState.ts:77-89`) tracks running state via
event types but has no timeout. The UI shows a perpetual "running"
state with no way out except manual interrupt.

## Goal

Add a frontend watchdog that detects stuck running state and offers
the user a recovery action.

## Behavior

1. When `agentRunning` becomes true, a timer starts (default 90s).
2. If no events arrive within the timeout, the UI shows a warning
   banner: "Agent seems unresponsive. No activity for {N}s."
3. The banner offers two actions:
   - "Wait" — resets the timer for another 90s
   - "Interrupt" — sends a cancel request to the session
4. If events resume, the timer clears silently.
5. The timeout is conservative — it should not fire during normal
   long-running tool execution, only when there's truly no activity.

## Dependencies

- None (frontend-only)

## Acceptance

- [x] Stuck running state detected after timeout
- [x] Warning banner with "Wait" and "Interrupt" actions
- [x] Timer resets on any new event
- [x] No false positives during normal long-running tools
- [x] Tests for timer logic
- [x] `make check` passes (frontend build/lint/test pass; no Rust changes)
