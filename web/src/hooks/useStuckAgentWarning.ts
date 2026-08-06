import { useCallback, useEffect, useRef, useState } from 'react'

/**
 * Default inactivity window before the stuck-agent warning surfaces.
 *
 * Conservative on purpose: normal long-running tool execution emits periodic
 * events (stream chunks, tool starts/finishes), so 90s of *zero* activity
 * strongly implies the agent is genuinely hung or disconnected — not merely
 * thinking. Callers may override via {@link UseStuckAgentWarningOptions.timeoutMs}.
 */
export const DEFAULT_STUCK_TIMEOUT_MS = 90_000

/**
 * Pure helper: milliseconds elapsed since the last activity baseline, clamped
 * to >= 0. Returns `null` when there is no baseline yet (e.g. the agent hasn't
 * started running or no event has ever arrived).
 *
 * Extracted as a pure function so the threshold math is unit-testable without
 * a React/DOM environment.
 */
export function elapsedSince(
  lastActivityMs: number | null,
  nowMs: number,
): number | null {
  if (lastActivityMs == null) return null
  return Math.max(0, nowMs - lastActivityMs)
}

/**
 * Pure helper: whether the elapsed inactivity exceeds the configured timeout.
 */
export function isStuck(elapsedMs: number | null, timeoutMs: number): boolean {
  return elapsedMs != null && elapsedMs >= timeoutMs
}

/**
 * Pure helper: whole seconds of inactivity to display in the warning banner
 * ("No activity for {N}s").
 */
export function stuckSeconds(elapsedMs: number | null): number {
  return elapsedMs == null ? 0 : Math.floor(elapsedMs / 1000)
}

/** Options passed into {@link useStuckAgentWarning}. */
interface UseStuckAgentWarningOptions {
  /** Whether the active session's agent turn is currently running. */
  agentRunning: boolean
  /** A monotonic signal that changes whenever a new event arrives for the
   *  active session (e.g. the last event's id, or the events array length).
   *  The hook records `Date.now()` as the inactivity baseline each time this
   *  changes, so callers never need to call `Date.now()` during render. Pass
   *  `null` when no event has arrived yet. */
  lastEventTimestamp: number | null
  /** Active session id. Switching sessions resets the watchdog. */
  sessionId: string
  /** Override the default 90s timeout (mainly for tests). */
  timeoutMs?: number
}

/** Return value of {@link useStuckAgentWarning}. */
interface UseStuckAgentWarningResult {
  /** True once the inactivity window elapses while running; false otherwise. */
  stuck: boolean
  /** Whole seconds since the last activity, for banner display. */
  seconds: number
  /** Reset the inactivity baseline for another full timeout window. */
  wait: () => void
  /** Hide the warning and reset the baseline (user dismissed it). */
  dismiss: () => void
}

/**
 * Frontend watchdog for a stuck `agentRunning` state.
 *
 * When the agent starts running, an inactivity baseline is recorded. If no new
 * event arrives within `timeoutMs` (default 90s), `stuck` becomes true so the
 * UI can surface a recovery banner. Any new event (detected via a change in
 * `lastEventTimestamp`) silently resets the baseline; `wait()` does the same
 * on user demand; `dismiss()` hides the banner and resets. When the agent
 * stops running, all state clears.
 *
 * State is only ever written from the 1s interval callback (an external timer
 * callback, which the react-hooks/set-state-in-effect rule permits) and from
 * the user-action callbacks — never synchronously inside an effect body. The
 * displayed seconds stay live because the tick re-renders once per second only
 * while the warning is actually showing.
 */
export function useStuckAgentWarning({
  agentRunning,
  lastEventTimestamp,
  sessionId,
  timeoutMs = DEFAULT_STUCK_TIMEOUT_MS,
}: UseStuckAgentWarningOptions): UseStuckAgentWarningResult {
  const [stuckRaw, setStuckRaw] = useState(false)
  const [seconds, setSeconds] = useState(0)

  // Wall-clock ms of the last activity (event arrival, running start, wait, or
  // dismiss). Null while the agent is idle so the watchdog doesn't arm.
  const activityMsRef = useRef<number | null>(null)

  // Reset the baseline when the agent stops or the session changes. Ref
  // mutations inside effects are fine; only synchronous setState is flagged.
  useEffect(() => {
    if (!agentRunning || !sessionId) {
      activityMsRef.current = null
    }
  }, [agentRunning, sessionId])

  // Arm the baseline when running begins (only if not already armed).
  useEffect(() => {
    if (agentRunning && activityMsRef.current == null) {
      activityMsRef.current = Date.now()
    }
  }, [agentRunning])

  // Any new event resets the inactivity baseline. The signal is a monotonic
  // value (event id / array length) supplied by the caller; we record Date.now()
  // here in the effect (impure calls are allowed in effects, unlike render) so
  // callers never need to call Date.now() during render. The next tick observes
  // the fresh baseline and clears `stuckRaw` on its own (no setState here).
  useEffect(() => {
    if (agentRunning && lastEventTimestamp != null) {
      activityMsRef.current = Date.now()
    }
  }, [lastEventTimestamp, agentRunning])

  // Tick once per second while running to check the threshold and keep the
  // displayed seconds live. All setState happens in this timer callback, which
  // is an external-system callback rather than a synchronous effect body.
  useEffect(() => {
    if (!agentRunning) return
    const id = window.setInterval(() => {
      const elapsed = elapsedSince(activityMsRef.current, Date.now())
      const stuckNow = isStuck(elapsed, timeoutMs)
      setStuckRaw(stuckNow)
      setSeconds(stuckNow ? stuckSeconds(elapsed) : 0)
    }, 1000)
    return () => window.clearInterval(id)
  }, [agentRunning, timeoutMs])

  const wait = useCallback(() => {
    activityMsRef.current = Date.now()
    setStuckRaw(false)
    setSeconds(0)
  }, [])

  const dismiss = useCallback(() => {
    // Treat dismiss like wait: hide the banner and reset the baseline so it
    // doesn't immediately re-fire on the next tick.
    activityMsRef.current = Date.now()
    setStuckRaw(false)
    setSeconds(0)
  }, [])

  // Derived so the banner vanishes the instant running stops, without needing
  // a setState in the stop effect. Stale `stuckRaw` clears on the next tick
  // once running resumes and the baseline is re-armed.
  return {
    stuck: agentRunning && stuckRaw,
    seconds,
    wait,
    dismiss,
  }
}
