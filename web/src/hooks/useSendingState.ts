import { useCallback, useEffect, useRef, useState } from 'react'
import type { AppEvent, Session } from '@/types'

/** Options passed into {@link useSendingState}. */
interface UseSendingStateOptions {
  /** Id of the currently active chat session, or null when none is active. */
  activeSessionId: string | null
  /** Events for the active session only (used to derive `lastEvent`). */
  events: AppEvent[]
  /** Events across all known sessions (used by the latch-clearing effect). */
  allEvents: AppEvent[]
  /** All currently known sessions (used to detect closed sessions). */
  sessions: Session[]
}

/** Return value of {@link useSendingState}. */
interface UseSendingStateResult {
  sendingSessionIds: string[]
  pendingNewChatSend: boolean
  setPendingNewChatSend: (v: boolean) => void
  clearSendingSession: (sessionId: string) => void
  markSendingSession: (sessionId: string, epoch: number) => void
  lastEvent: AppEvent | undefined
  isRunningEvent: (e: AppEvent | undefined) => boolean
  isTerminalEvent: (e: AppEvent | undefined) => boolean
  activeSending: boolean
}

/**
 * Owns the per-session admission/streaming latches that previously lived inside
 * `ChatPanel`.
 *
 * POST /prompt returns after a turn is admitted (not after it completes), so a
 * single boolean latch would lock every chat's composer while any one turn
 * runs. Instead we track each in-flight session id in `sendingSessionIds` and
 * release it once that session's terminal event lands (or the session
 * disappears). The active session's composer is locked only when its own
 * latch is set or while a brand-new chat is being created on first send.
 *
 * @param opts Inputs from the host component.
 * @returns Latch state, mutators, derived running flags, and event helpers.
 */
export function useSendingState({
  activeSessionId,
  events,
  allEvents,
  sessions,
}: UseSendingStateOptions): UseSendingStateResult {
  // Per-session admission latches. POST /prompt returns after the turn is
  // admitted, so we keep each session id here until its terminal event lands.
  // A single boolean would lock every chat's composer while any one turn runs.
  const [sendingSessionIds, setSendingSessionIds] = useState<string[]>([])
  // True only while creating a brand-new session (no id yet) on first send.
  const [pendingNewChatSend, setPendingNewChatSend] = useState(false)
  // Highest known event id at send time, keyed by session, so a prior turn's
  // terminal StreamUpdate cannot clear the latch immediately.
  const sendEpochBySessionRef = useRef<Map<string, number>>(new Map())

  const clearSendingSession = useCallback((sessionId: string) => {
    setSendingSessionIds((prev) =>
      prev.includes(sessionId) ? prev.filter((id) => id !== sessionId) : prev,
    )
    sendEpochBySessionRef.current.delete(sessionId)
  }, [])

  const markSendingSession = useCallback((sessionId: string, epoch: number) => {
    sendEpochBySessionRef.current.set(sessionId, epoch)
    setSendingSessionIds((prev) =>
      prev.includes(sessionId) ? prev : [...prev, sessionId],
    )
  }, [])

  // Per-session turn-in-flight helpers. The composer for the *active* session
  // is locked only when that session is sending/streaming; other sessions stay
  // free so concurrent chats can be composed independently.
  const lastEvent = events[events.length - 1]
  const isRunningEvent = (e: AppEvent | undefined): boolean =>
    !!e &&
    ((e.type === 'StreamUpdate' && !!e.streaming) ||
      e.type === 'PromptSubmitted' ||
      e.type === 'ResponseStarted' ||
      e.type === 'ToolStarted' ||
      e.type === 'ShellCommandStarted')
  const isTerminalEvent = (e: AppEvent | undefined): boolean =>
    !!e &&
    ((e.type === 'StreamUpdate' && !e.streaming) ||
      e.type === 'SessionCancelled' ||
      e.type === 'SessionInterrupted' ||
      e.type === 'AgentExited')

  // Active session only: local admission latch OR live streaming events.
  // Switching away does not keep the composer locked for the newly active chat.
  const activeSending =
    pendingNewChatSend ||
    (!!activeSessionId && sendingSessionIds.includes(activeSessionId))

  // Clear each session's admission latch when its terminal event arrives, or
  // when the session no longer exists (closed locally or via multi-client sync).
  // Scans allEvents so background sessions complete without being active.
  useEffect(() => {
    if (sendingSessionIds.length === 0) return
    const known = new Set(sessions.map((s) => s.id))
    let changed = false
    const next = sendingSessionIds.filter((sessionId) => {
      if (!known.has(sessionId)) {
        sendEpochBySessionRef.current.delete(sessionId)
        changed = true
        return false
      }
      const epoch = sendEpochBySessionRef.current.get(sessionId) ?? 0
      const sessionEvents = allEvents.filter((e) => e.sessionId === sessionId)
      const last = sessionEvents[sessionEvents.length - 1]
      if (last && (last.id ?? 0) > epoch && isTerminalEvent(last)) {
        sendEpochBySessionRef.current.delete(sessionId)
        changed = true
        return false
      }
      return true
    })
    if (changed) setSendingSessionIds(next)
  }, [allEvents, sendingSessionIds, sessions])

  return {
    sendingSessionIds,
    pendingNewChatSend,
    setPendingNewChatSend,
    clearSendingSession,
    markSendingSession,
    lastEvent,
    isRunningEvent,
    isTerminalEvent,
    activeSending,
  }
}
