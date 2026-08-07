import { useCallback, useRef, type MutableRefObject } from 'react'
import { api, type WorkspaceInfo, type UploadResult, type EditorSelectionInfo } from '@/lib/api'
import { describeSessionError, SESSION_GONE_MESSAGE } from '@/lib/errors'
import type { AppEvent, Attachment, Session } from '@/types'
import { safeStorage } from '@/lib/safeStorage'

interface UseSessionActionsOptions {
  activeWorkspaceRef: MutableRefObject<WorkspaceInfo | null>
  setSessions: React.Dispatch<React.SetStateAction<Session[]>>
  loadSessions: () => Promise<void>
  commitEvents: (next: AppEvent[] | ((prev: AppEvent[]) => AppEvent[])) => void
  eventsRef: MutableRefObject<AppEvent[]>
}

interface UseSessionActionsResult {
  createSession: (agentId: string, modelId: string, profileId?: string) => Promise<Session>
  sendPrompt: (sessionId: string, content: string, attachments?: Attachment[]) => Promise<void>
  uploadFile: (sessionId: string, file: File) => Promise<UploadResult>
  cancelSession: (sessionId: string) => Promise<void>
  renameSession: (sessionId: string, name: string) => Promise<void>
  rebindSession: (
    sessionId: string,
    agentId: string,
    modelId: string,
    maxTransferBytes?: number,
  ) => Promise<void>
  switchModel: (sessionId: string, modelId: string) => Promise<void>
  reportContext: (
    sessionId: string,
    openFiles: string[],
    recentEdits: string[],
    selection?: EditorSelectionInfo,
  ) => void
  deleteSession: (sessionId: string) => Promise<void>
  exportSession: (sessionId: string) => Promise<void>
  /** The reportContext debounce timer ref — must be cleared on unmount by the caller. */
  reportContextTimerRef: MutableRefObject<ReturnType<typeof setTimeout> | undefined>
}

/**
 * Owns the session REST action callbacks (create / prompt / patch / delete /
 * export / context-report) that were previously inlined into `useBackend`.
 *
 * Extracted as a standalone hook so `useBackend` stays under a maintainable
 * size. The hook is pure plumbing: it receives the workspace ref, the session
 * list setter, a reload function, and the event-log commit/events refs as
 * inputs, and returns callbacks that mutate state through those inputs. It does
 * not own any state of its own except the `reportContext` debounce timer.
 *
 * @param opts Injected dependencies from the host hook.
 * @returns The session action callbacks plus the reportContext timer ref.
 */
export function useSessionActions(opts: UseSessionActionsOptions): UseSessionActionsResult {
  const { activeWorkspaceRef, setSessions, loadSessions, commitEvents, eventsRef } = opts

  // ---- Session actions ----
  const createSession = useCallback(
    async (agentId: string, modelId: string, profileId?: string) => {
      const wsId = activeWorkspaceRef.current?.id || ''
      const session = await api.createSession(agentId, modelId, wsId, profileId)
      setSessions((prev) => [...prev, session])
      return session
    },
    [activeWorkspaceRef, setSessions],
  )

  const sendPrompt = useCallback(
    async (sessionId: string, content: string, attachments?: Attachment[]) => {
      // Check if this is the first prompt (session still has default name).
      // The backend auto-names on first prompt; we need to refresh the session
      // list so the new name shows up without a page reload.
      let wasNewChat = false
      setSessions((prev) => {
        wasNewChat = prev.some((s) => s.id === sessionId && s.name === 'New chat')
        return prev
      })

      try {
        await api.sendPrompt(sessionId, content, attachments)
      } catch (err) {
        // A stale activeSessionId (e.g. after a daemon restart that wiped
        // conversations.json, or a deleted session) makes the backend return
        // 404 "session not found: sess-…". Recover gracefully: clear the
        // persisted id so the UI resets to the new-chat state, and surface a
        // friendly message instead of the raw error string.
        const { sessionGone } = describeSessionError(err, String(err))
        if (sessionGone) {
          safeStorage.remove('lai:activeSessionId')
          throw new Error(SESSION_GONE_MESSAGE, { cause: err })
        }
        throw err
      }

      // Refresh session list so the auto-generated name appears in the UI.
      // Cross-device rename sync will be handled by a future SessionRenamed
      // event (see pending-per-workspace-tabs story).
      if (wasNewChat) {
        await loadSessions()
      }
    },
    [setSessions, loadSessions],
  )

  /** Uploads a file to a session's upload store. Thin wrapper around
   *  api.uploadFile so components can call it through the hook and share the
   *  hook's session-recovery semantics. */
  const uploadFile = useCallback(
    async (sessionId: string, file: File): Promise<UploadResult> => {
      return await api.uploadFile(sessionId, file)
    },
    [],
  )

  const cancelSession = useCallback(async (sessionId: string) => {
    await api.cancelSession(sessionId)
  }, [])

  const renameSession = useCallback(
    async (sessionId: string, name: string) => {
      await api.patchSession(sessionId, { name })
      await loadSessions()
    },
    [loadSessions],
  )

  const rebindSession = useCallback(
    async (
      sessionId: string,
      agentId: string,
      modelId: string,
      maxTransferBytes?: number,
    ) => {
      await api.patchSession(sessionId, { agentId, modelId, maxTransferBytes })
      await loadSessions()
    },
    [loadSessions],
  )

  /**
   * Switches the model on a live session without restarting the agent process.
   * Unlike rebindSession, this preserves the full conversation context — the
   * agent keeps its in-memory state and just uses the new model for subsequent
   * turns. Sends a model-only PATCH (no agentId) so the backend routes to
   * SwitchModel instead of RebindSession.
   */
  const switchModel = useCallback(
    async (sessionId: string, modelId: string) => {
      await api.patchSession(sessionId, { modelId })
      await loadSessions()
    },
    [loadSessions],
  )

  // Debounce timer for reportContext so rapid tab switches / edits don't
  // flood the backend with context updates.
  const reportContextTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  /**
   * Reports the current open files and recent edits to the backend so the
   * context middleware can inject them into the next agent prompt. Debounced
   * by ~1s to coalesce rapid tab switches and keystroke-driven unsaved-state
   * changes into a single request. The optional selection is the user's
   * current editor selection (sent as a resource block by the backend).
   */
  const reportContext = useCallback(
    (
      sessionId: string,
      openFiles: string[],
      recentEdits: string[],
      selection?: EditorSelectionInfo,
    ) => {
      if (reportContextTimerRef.current) clearTimeout(reportContextTimerRef.current)
      reportContextTimerRef.current = setTimeout(async () => {
        try {
          await api.reportSessionContext(sessionId, openFiles, recentEdits, selection)
        } catch {
          // Non-fatal — context reporting is best-effort.
        }
      }, 1000)
    },
    [],
  )

  const deleteSession = useCallback(
    async (sessionId: string) => {
      await api.closeSession(sessionId)
      setSessions((prev) => prev.filter((s) => s.id !== sessionId))
      // Drop the deleted conversation's events from the local cache. Filtering
      // only shrinks the list, but route it through commitEvents anyway so every
      // event-log mutation goes through the single capped path.
      commitEvents(eventsRef.current.filter((e) => e.sessionId !== sessionId))
    },
    [commitEvents, eventsRef, setSessions],
  )

  /** Exports a conversation as a markdown transcript. The backend renders the
   *  full event history into a readable transcript and the api client triggers
   *  a browser download of the resulting text/markdown blob. */
  const exportSession = useCallback(async (sessionId: string) => {
    await api.exportSession(sessionId)
  }, [])

  return {
    createSession,
    sendPrompt,
    uploadFile,
    cancelSession,
    renameSession,
    rebindSession,
    switchModel,
    reportContext,
    deleteSession,
    exportSession,
    reportContextTimerRef,
  }
}
