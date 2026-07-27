import { useCallback, useState } from 'react'
import { useLocalStorage } from '@/hooks/useLocalStorage'
import type { Session } from '@/types'

interface UseChatTabsOptions {
  sessions: Session[]
  activeSessionId: string | null
  onSelectSession: (sessionId: string) => void
}

/**
 * Manages persisted chat-tab ids, reconciles them with available sessions,
 * auto-opens the active session, and selects a fallback when a tab closes.
 */
export function useChatTabs({
  sessions,
  activeSessionId,
  onSelectSession,
}: UseChatTabsOptions) {
  const [openTabIds, setOpenTabIds] = useLocalStorage<string[]>('lai:openTabIds', [])

  // Auto-open the active session as a tab. New chats and restored chats would
  // otherwise never enter openTabIds, so they would not render a tab.
  const [prevActiveSessionId, setPrevActiveSessionId] = useState(activeSessionId)
  if (activeSessionId !== prevActiveSessionId) {
    setPrevActiveSessionId(activeSessionId)
    if (activeSessionId && sessions.some((session) => session.id === activeSessionId)) {
      setOpenTabIds((currentIds) =>
        currentIds.includes(activeSessionId)
          ? currentIds
          : [...currentIds, activeSessionId],
      )
    }
  }

  // Drop ids whose sessions were deleted and cover the cold-reload case where
  // the active id is restored before the asynchronous session list arrives.
  const [prevSessions, setPrevSessions] = useState(sessions)
  if (sessions !== prevSessions) {
    setPrevSessions(sessions)
    const knownIds = new Set(sessions.map((session) => session.id))
    setOpenTabIds((currentIds) => {
      const filteredIds = currentIds.filter((id) => knownIds.has(id))
      if (filteredIds.length === currentIds.length) {
        // Nothing removed — only ensure the active id is present if known.
        if (
          activeSessionId &&
          knownIds.has(activeSessionId) &&
          !currentIds.includes(activeSessionId)
        ) {
          return [...currentIds, activeSessionId]
        }
        return currentIds
      }
      if (
        activeSessionId &&
        knownIds.has(activeSessionId) &&
        !filteredIds.includes(activeSessionId)
      ) {
        return [...filteredIds, activeSessionId]
      }
      return filteredIds
    })
  }

  // Stable callbacks: SessionCreated/Closed effects depend on these and must
  // not re-fire every render (an unstable handleCloseTab + sticky closed id
  // previously looped via always-new filter arrays thrashing localStorage).
  const openTab = useCallback((id: string) => {
    setOpenTabIds((currentIds) =>
      currentIds.includes(id) ? currentIds : [...currentIds, id],
    )
  }, [setOpenTabIds])

  const handleCloseTab = useCallback((id: string) => {
    setOpenTabIds((currentIds) => {
      if (!currentIds.includes(id)) return currentIds
      const nextIds = currentIds.filter((tabId) => tabId !== id)
      if (id === activeSessionId) {
        onSelectSession(nextIds.length > 0 ? nextIds[nextIds.length - 1] : '')
      }
      return nextIds
    })
  }, [activeSessionId, onSelectSession, setOpenTabIds])

  return { openTabIds, openTab, handleCloseTab }
}
