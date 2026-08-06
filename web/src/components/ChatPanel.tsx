import { useCallback, useEffect, useState, useRef, useMemo, type ChangeEvent, type CSSProperties } from 'react'
import { WifiOff, AlertCircle, X, Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'
import {
  api,
  type SessionHistoryCapabilities,
  type UploadResult,
} from '@/lib/api'
import { isSessionNotFound } from '@/lib/errors'
import { useRetrying } from '@/lib/retry'
import { ChatTabBar } from './ChatTabBar'
import { ChatComposer } from './ChatComposer'
import { AssistantThread } from './chat/AssistantThread'
import { ChatHistory } from './ChatHistory'
import { SwitchAgentDialog } from './SwitchAgentDialog'
import { WorkspaceBar } from './chat/WorkspaceBar'
import { Banner } from './ui/Banner'
import { useChatTabs } from '@/hooks/useChatTabs'
import { useMcpServers } from '@/hooks/useMcpServers'
import { useAgentSelection } from '@/hooks/useAgentSelection'
import { useProfileSelection } from '@/hooks/useProfileSelection'
import { type ContextUsage } from '@/lib/contextUsage'
import { useSendingState } from '@/hooks/useSendingState'
import { useStuckAgentWarning } from '@/hooks/useStuckAgentWarning'
import { pushRecentModel } from '@/lib/modelPrefs'
import type { AppEvent, Agent, Attachment, Session } from '@/types'
import type { PendingPermission } from '@/lib/api'

export interface ChatPanelActions {
  onSendMessage: (sessionId: string, content: string, attachments?: Attachment[]) => Promise<void>
  onCreateSession: (agentId: string, modelId: string, profileId?: string) => Promise<string>
  onPermissionResponse: (requestId: string, decision: string) => void
  onSelectSession: (sessionId: string) => void
  onCancel: (sessionId: string) => void
  onRenameSession: (sessionId: string, name: string) => void
  onDeleteSession: (sessionId: string) => void
  onRebindSession: (sessionId: string, agentId: string, modelId: string, maxTransferBytes?: number) => void
  /** Switches the model on a live session without restarting the agent process
   *  (preserves conversation history). Used by handleModelChange for model-only
   *  switches; onRebindSession is still used for agent (harness) switches. */
  onSwitchModel: (sessionId: string, modelId: string) => Promise<void>
  onExportSession: (sessionId: string) => void
  onLoadOlder: (sessionId: string) => Promise<void>
  /** Uploads a file to a session's upload store. Routed through useBackend so
   *  uploads share the hook's session-recovery semantics instead of bypassing
   *  it via api.uploadFile directly. */
  onUploadFile: (sessionId: string, file: File) => Promise<UploadResult>
  /** Workspace change handler. */
  onSelectWorkspace: (id: string) => void
}

/**
 * Right sidebar — agent chat (Blueprint Sec 17 — right sidebar).
 *
 * Slim orchestrator after the WI-3 restructure: owns agent/model state +
 * persistence, the switch-agent dialog, autoscroll, history popout open
 * state, input/attachment/sending/uploading state, and the open-tab ids.
 * Header markup lives in `ChatTabBar`, message rendering in `ConversationView`,
 * input markup in `ChatComposer`.
 *
 * On desktop: always visible alongside editor.
 * On mobile: full-screen via bottom nav.
 */
export function ChatPanel({
  events,
  allEvents,
  agents,
  sessions,
  workspaces,
  visible,
  connected,
  isDesktop,
  pendingPermissions,
  activeSessionId,
  pendingCreatedSessionIds,
  pendingClosedSessionIds,
  hasOlderEvents,
  onConsumeSessionCreated,
  onConsumeSessionClosed,
  actions,
  workspaceId,
  style,
  onOpenMcpSettings,
}: {
  events: AppEvent[]
  /** All events across all sessions — used only to compute the running
   *  indicator for non-active open tabs. The active session's running state
   *  is derived from `events` (already filtered to it) plus the per-session
   *  sending latch. Don't use this for conversation rendering. */
  allEvents: AppEvent[]
  agents: Agent[]
  sessions: Session[]
  workspaces: { id: string; name: string }[]
  visible: boolean
  connected: boolean
  isDesktop: boolean
  pendingPermissions: PendingPermission[]
  activeSessionId: string | null
  /** Queued ids of sessions another client created (multi-client sync). */
  pendingCreatedSessionIds: string[]
  /** Queued ids of sessions another client closed (multi-client sync). */
  pendingClosedSessionIds: string[]
  /** Whether the active session has another older history page. */
  hasOlderEvents: boolean
  /** Drain one SessionCreated id after the tab has been opened (or skipped). */
  onConsumeSessionCreated: (sessionId: string) => void
  /** Drain one SessionClosed id after the tab has been closed. */
  onConsumeSessionClosed: (sessionId: string) => void
  actions: ChatPanelActions
  /** Active workspace id. */
  workspaceId: string
  /** Optional inline style — used by App.tsx to apply a persisted panel width on desktop. */
  style?: CSSProperties
  /**
   * Opens app Settings focused on the MCP Servers section. Forwarded from
   * App → ChatComposer → McpPopout Settings icon.
   */
  onOpenMcpSettings?: () => void
}) {
  const {
    onSendMessage,
    onCreateSession,
    onPermissionResponse,
    onSelectSession,
    onCancel,
    onRenameSession,
    onDeleteSession,
    onRebindSession,
    onSwitchModel,
    onExportSession,
    onLoadOlder,
    onUploadFile,
    onSelectWorkspace,
  } = actions

  const [chatHistoryOpen, setChatHistoryOpen] = useState(false)
  const [historyCapabilityState, setHistoryCapabilityState] = useState<{
    sessionId: string
    caps?: SessionHistoryCapabilities
  }>()
  const [input, setInput] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [loadingOlderEvents, setLoadingOlderEvents] = useState(false)

  // Subtle "Retrying…" indicator — true while a mutating API call (send
  // prompt, upload, model/profile switch) is in a backoff sleep before a
  // retry attempt. See web/src/lib/retry.ts.
  const retrying = useRetrying()

  // Transient "New chat" placeholder tab. Set true by handleNewChat so the tab
  // bar shows a "New chat" tab immediately (without a backend round-trip).
  // Cleared once a real session becomes active (via send/upload) or another
  // tab is selected. This keeps new-tab creation instant — the actual session
  // is created lazily on first send/upload by handleSend/handleFileChange.
  const [showNewChatTab, setShowNewChatTab] = useState(false)

  // Pending image attachments for the next prompt. `pendingAttachments` holds
  // the {id, name, mimeType} sent with the prompt; `pendingPreviews` holds the
  // {url, name} used to render thumbnails before the message is sent.
  const [pendingAttachments, setPendingAttachments] = useState<Attachment[]>([])
  const [pendingPreviews, setPendingPreviews] = useState<{ url: string; name: string }[]>([])
  const [uploading, setUploading] = useState(false)
  const [uploadError, setUploadError] = useState<string | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const {
    mcpServers,
    mcpHealth,
    mcpStatusLoading,
    mcpTogglingServer,
    mcpConfigChanged,
    setMcpConfigChanged,
    mcpError,
    setMcpError,
    loadMcpStatus,
    handleToggleMcpServer,
  } = useMcpServers()

  const {
    profiles,
    selectedProfileId,
    profileOverride,
    setProfileOverride,
    handleProfileChange,
  } = useProfileSelection({ activeSessionId, onSelectSession, setError })

  /** Restart for MCP: ACP doesn't support live add/remove in v1, so the
   *  simplest correct action is to drop the active session and let the user
   *  start a new chat, which will pick up the updated MCP config. */
  const handleRestartForMcp = () => {
    setMcpConfigChanged(false)
    onSelectSession('')
    setShowNewChatTab(true)
  }

  const { openTabIds, openTab, handleCloseTab } = useChatTabs({
    sessions,
    activeSessionId,
    onSelectSession,
  })

  // Multi-client session-list sync (Blueprint Sec 12). Drain create/close
  // queues one id at a time. openTab never steals focus; handleCloseTab picks
  // a fallback only if the closed id was active. Unknown ids stay queued until
  // loadSessions repopulates (creates) or are dropped after tab close (closes).
  // If a created session was deleted before this client's loadSessions returned
  // it, the id never appears in `sessions` — drop it once the list has loaded
  // so the queue drains and the parent can clear its own copy.
  useEffect(() => {
    if (pendingCreatedSessionIds.length === 0) return
    for (const id of pendingCreatedSessionIds) {
      if (sessions.some((s) => s.id === id)) {
        openTab(id)
        onConsumeSessionCreated(id)
      } else if (sessions.length > 0) {
        onConsumeSessionCreated(id)
      }
    }
  }, [pendingCreatedSessionIds, sessions, openTab, onConsumeSessionCreated])

  useEffect(() => {
    if (pendingClosedSessionIds.length === 0) return
    for (const id of pendingClosedSessionIds) {
      handleCloseTab(id)
      onConsumeSessionClosed(id)
    }
  }, [pendingClosedSessionIds, handleCloseTab, onConsumeSessionClosed])

  const hasConversation =
    !!activeSessionId && events.some((e) => e.type === 'PromptSubmitted')

  const {
    effectiveAgentId,
    effectiveModelId,
    currentAgent,
    userSelectedModel,
    activeSession,
    pendingAgentId,
    truncateLength,
    setTruncateLength,
    rebinding,
    handleAgentChange,
    confirmSwitchAgent,
    cancelSwitchAgent,
    handleModelChange,
  } = useAgentSelection({
    agents,
    sessions,
    activeSessionId,
    hasConversation,
    onRebindSession,
  })

  useEffect(() => {
    let cancelled = false
    if (!activeSessionId || !chatHistoryOpen) {
      return () => { cancelled = true }
    }
    void api.getSessionHistoryCapabilities(activeSessionId)
      .then((caps) => {
        if (!cancelled) setHistoryCapabilityState({ sessionId: activeSessionId, caps })
      })
      .catch(() => {
        if (!cancelled) setHistoryCapabilityState({ sessionId: activeSessionId })
      })
    return () => { cancelled = true }
  }, [activeSessionId, chatHistoryOpen])

  const {
    sendingSessionIds,
    setPendingNewChatSend,
    clearSendingSession,
    markSendingSession,
    lastEvent,
    isRunningEvent,
    activeSending,
  } = useSendingState({ activeSessionId, events, allEvents, sessions })

  const agentRunning = activeSending || isRunningEvent(lastEvent)

  // Monotonic activity signal for the stuck-agent watchdog: changes whenever a
  // new event lands for the active session (event id when present, else the
  // events array length). AppEvent has no timestamp field, and Date.now() must
  // not run during render, so the hook records the wall-clock baseline itself
  // in an effect each time this signal changes.
  const lastEventTimestamp = useMemo(
    () => (lastEvent ? (lastEvent.id ?? events.length) : null),
    [lastEvent, events.length],
  )

  // Frontend watchdog: if agentRunning stays true with no events for 90s,
  // surface a recovery banner. See useStuckAgentWarning for the full contract.
  const { stuck: stuckAgent, seconds: stuckSeconds, wait: waitStuckAgent } =
    useStuckAgentWarning({
      agentRunning,
      lastEventTimestamp,
      sessionId: activeSessionId ?? '',
    })

  // Per-tab running dots: admission latch and/or last event per open session.
  const runningSessionIds = useMemo(() => {
    const ids = new Set<string>(sendingSessionIds)
    // Group allEvents' last event per session in one pass instead of filtering per tab.
    const lastBySession = new Map<string, AppEvent>()
    for (const e of allEvents) {
      lastBySession.set(e.sessionId, e)
    }
    for (const id of openTabIds) {
      if (isRunningEvent(lastBySession.get(id))) ids.add(id)
    }
    if (activeSessionId && isRunningEvent(lastEvent)) {
      ids.add(activeSessionId)
    }
    return ids
  }, [sendingSessionIds, lastEvent, openTabIds, allEvents, activeSessionId, isRunningEvent])

  // Tabs to render — sessions whose id is in openTabIds, in openTabIds order.
  const openTabs = openTabIds
    .map((id) => sessions.find((s) => s.id === id))
    .filter((s): s is Session => !!s)

  const handleSend = async () => {
    const content = input.trim()
    // Only block when *this* session already has an in-flight turn. Other
    // sessions may be streaming in the background without locking the composer.
    if (
      (!content && pendingAttachments.length === 0) ||
      activeSending ||
      uploading ||
      !effectiveAgentId ||
      !effectiveModelId
    ) {
      return
    }

    setError(null)
    setInput('')

    // Snapshot max known id for this session before the turn's events arrive.
    const epochFor = (sessionId: string | null) => {
      const source = sessionId
        ? allEvents.filter((e) => e.sessionId === sessionId)
        : events
      return source.reduce((max, e) => Math.max(max, e.id ?? 0), 0)
    }

    let sessionId = activeSessionId
    const wasNewSession = !sessionId
    if (wasNewSession) {
      setPendingNewChatSend(true)
    } else if (sessionId) {
      markSendingSession(sessionId, epochFor(sessionId))
    }

    try {
      if (!sessionId) {
        sessionId = await onCreateSession(effectiveAgentId, effectiveModelId, selectedProfileId)
        setShowNewChatTab(false)
        if (profileOverride?.sessionId === '') {
          const pendingProfileId = profileOverride.profileId
          setProfileOverride({ sessionId, profileId: pendingProfileId })
          try {
            localStorage.setItem(`local-agent:profile:${sessionId}`, pendingProfileId)
          } catch {
            // Ignore write failures (quota / disabled storage).
          }
        }
        // Promote the pending-new-chat latch onto the real session id.
        setPendingNewChatSend(false)
        markSendingSession(sessionId, epochFor(sessionId))
      }
      openTab(sessionId)
      if (!wasNewSession && userSelectedModel && activeSession?.modelId !== userSelectedModel) {
        await onSwitchModel(sessionId, userSelectedModel)
      }
      const attachmentsToSend = pendingAttachments
      await onSendMessage(
        sessionId,
        content,
        attachmentsToSend.length > 0 ? attachmentsToSend : undefined,
      )
      if (effectiveAgentId && effectiveModelId) {
        pushRecentModel(effectiveAgentId, effectiveModelId)
      }
      setPendingAttachments([])
      setPendingPreviews([])
      // Keep the per-session latch until a terminal event clears it.
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to send message'
      if (isSessionNotFound(message)) {
        setError('This conversation is no longer available. Start a new chat.')
        onSelectSession('')
      } else {
        setError(message)
        // Only restore if the composer is still empty — the user may have
        // typed new text while the in-flight send was failing.
        setInput((current) => (current === '' ? content : current))
      }
      setPendingNewChatSend(false)
      if (sessionId) clearSendingSession(sessionId)
    }
  }

  const handleStop = () => {
    // Cancel only the active session — background turns keep running.
    if (activeSessionId) {
      onCancel(activeSessionId)
      clearSendingSession(activeSessionId)
    }
    setPendingNewChatSend(false)
  }

  /** Opens the native file picker for image attachments. */
  const handlePickFiles = () => {
    setUploadError(null)
    fileInputRef.current?.click()
  }

  /** Uploads each selected image and appends it to the pending attachments.
   *  Creates a session on demand if the user is in the "new chat" state, since
   *  uploads require an existing session id. */
  const handleFileChange = async (e: ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files || files.length === 0) return
    e.target.value = ''

    if (!effectiveAgentId || !effectiveModelId) return
    setUploading(true)
    setUploadError(null)
    try {
      let sessionId = activeSessionId
      if (!sessionId) {
        sessionId = await onCreateSession(effectiveAgentId, effectiveModelId, selectedProfileId)
        // A real session now exists — drop the transient "New chat" placeholder.
        setShowNewChatTab(false)
      }
      // Always ensure the active session is opened as a tab (see handleSend).
      openTab(sessionId)
      for (const file of Array.from(files)) {
        const result = await onUploadFile(sessionId, file)
        setPendingAttachments((prev) => [
          ...prev,
          { id: result.id, name: result.name, mimeType: result.mimeType },
        ])
        setPendingPreviews((prev) => [...prev, { url: result.url, name: result.name }])
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to upload file'
      if (isSessionNotFound(message)) {
        setError('This conversation is no longer available. Start a new chat.')
        onSelectSession('')
      } else {
        setUploadError(message)
      }
    } finally {
      setUploading(false)
    }
  }

  /** Removes a pending attachment by index. */
  const removePendingAttachment = (index: number) => {
    setPendingAttachments((prev) => prev.filter((_, i) => i !== index))
    setPendingPreviews((prev) => prev.filter((_, i) => i !== index))
  }

  // Map permission request IDs to their resolution so resolved cards collapse.
  const permissionResolution = useMemo(() => {
    const m = new Map<string, 'granted' | 'denied'>()
    for (const e of events) {
      if (e.requestId && (e.type === 'PermissionGranted' || e.type === 'PermissionDenied' || e.type === 'PermissionTimedOut')) {
        m.set(e.requestId, e.type === 'PermissionGranted' ? 'granted' : 'denied')
      }
    }
    return m
  }, [events])

  // Latest context-usage for the active session, from UsageUpdated events.
  // ACP agents emit usage_update as the context window fills; we take the
  // last one so the ring reflects the current state.
  const contextUsage = useMemo<ContextUsage | null>(() => {
    let latest: ContextUsage | null = null
    for (const e of events) {
      if (e.type === 'UsageUpdated' && e.tokensUsed !== undefined && e.tokensSize !== undefined) {
        latest = {
          used: e.tokensUsed,
          size: e.tokensSize,
          costAmount: e.costAmount,
          costCurrency: e.costCurrency,
        }
      }
    }
    return latest
  }, [events])

  const canSend = Boolean(
    (input.trim() || pendingAttachments.length > 0) &&
      effectiveAgentId &&
      effectiveModelId &&
      !activeSending &&
      !uploading,
  )

  /**
   * Merges consecutive StreamUpdate events into a single accumulated message
   * so streaming text appears as one growing response (like ChatGPT).
   * Also folds ShellOutputStreamed chunks into the preceding ShellCommandStarted
   * card (and preserves that output when Completed replaces Started).
   */
  const mergedEvents: AppEvent[] = useMemo(
    () =>
      events.reduce((acc: AppEvent[], event: AppEvent) => {
        if (event.type === 'StreamUpdate') {
          const last = acc[acc.length - 1]
          if (
            last &&
            last.type === 'StreamUpdate' &&
            last.role === event.role &&
            !!last.thought === !!event.thought
          ) {
            acc[acc.length - 1] = {
              ...last,
              content: (last.content || '') + (event.content || ''),
              streaming: event.streaming,
            }
            return acc
          }
        }
        // Live shell stdout/stderr: append onto the running Started card.
        if (event.type === 'ShellOutputStreamed') {
          let startedIdx = -1
          for (let i = acc.length - 1; i >= 0; i--) {
            const e = acc[i]
            if (e.type === 'ShellCommandStarted' && (!event.toolCallId || e.toolCallId === event.toolCallId)) {
              startedIdx = i
              break
            }
          }
          if (startedIdx !== -1) {
            const started = acc[startedIdx]
            acc[startedIdx] = {
              ...started,
              content: (started.content || '') + (event.content || ''),
            }
            return acc
          }
          // Orphan chunk (no matching Started) — drop, matching prior UI behavior.
          return acc
        }
        // Completed replaces Started so exit code + streamed output share one card.
        if (event.type === 'ShellCommandCompleted') {
          let startedIdx = -1
          for (let i = acc.length - 1; i >= 0; i--) {
            const e = acc[i]
            if (e.type === 'ShellCommandStarted' && (!event.toolCallId || !e.toolCallId || e.toolCallId === event.toolCallId)) {
              startedIdx = i
              break
            }
          }
          if (startedIdx !== -1) {
            const started = acc[startedIdx]
            acc[startedIdx] = {
              ...event,
              id: started.id, // Preserve original event ID for stable React keys
              content: started.content || event.content || event.summary,
            }
            return acc
          }
        }
        // ToolCompleted replaces ToolStarted so the tool card transitions from
        // running to complete in place (same pattern as ShellCommand above).
        // ToolCompleted doesn't carry `command`/`tool`/`target` (those are on
        // ToolStarted), so preserve them from the Started event to keep the
        // completed card's args/label intact.
        if (event.type === 'ToolCompleted') {
          let startedIdx = -1
          for (let i = acc.length - 1; i >= 0; i--) {
            const e = acc[i]
            if (e.type === 'ToolStarted' && (!event.toolCallId || !e.toolCallId || e.toolCallId === event.toolCallId)) {
              startedIdx = i
              break
            }
          }
          if (startedIdx !== -1) {
            const started = acc[startedIdx]
            acc[startedIdx] = {
              ...event,
              id: started.id, // Preserve original event ID for stable React keys
              toolCallId: event.toolCallId ?? started.toolCallId,
              command: event.command ?? started.command,
              tool: event.tool ?? started.tool,
              target: event.target ?? started.target,
              toolKind: event.toolKind ?? started.toolKind,
            }
            return acc
          }
        }
        acc.push(event)
        return acc
      }, []),
    [events],
  )

  const handleLoadOlder = useCallback(async () => {
    if (!activeSessionId || loadingOlderEvents) return
    setLoadingOlderEvents(true)
    try {
      await onLoadOlder(activeSessionId)
    } finally {
      setLoadingOlderEvents(false)
    }
  }, [activeSessionId, loadingOlderEvents, onLoadOlder])

  const handleNewChat = () => {
    // Lazy session creation: just reset transient UI state and drop into the
    // new-chat state (activeSessionId = null). The actual backend session is
    // created on first send/upload by handleSend/handleFileChange, which both
    // already handle the null-activeSessionId case. This keeps "+" instant —
    // no agent-process spawn / ACP handshake round-trip on tab creation.
    // Show a transient "New chat" placeholder tab so the user sees feedback.
    setChatHistoryOpen(false)
    setError(null)
    setInput('')
    setShowNewChatTab(true)
    onSelectSession('')
  }

  /** Selecting a session from a tab or the history popout also opens it as a tab. */
  const handleSelectAndOpen = (id: string) => {
    if (id) {
      openTab(id)
      // Selecting a real session dismisses the transient "New chat" placeholder.
      setShowNewChatTab(false)
    }
    onSelectSession(id)
    setChatHistoryOpen(false)
  }

  return (
    <aside
      className={cn(
        'flex-col h-full shrink-0 w-full bg-background border-l border-border lg:w-96',
        visible ? 'flex' : 'hidden',
        'absolute inset-0 z-30 lg:relative lg:inset-auto lg:z-auto',
      )}
      style={style}
    >
      <ChatTabBar
        openTabs={openTabs}
        activeSessionId={activeSessionId}
        runningSessionIds={runningSessionIds}
        onSelectSession={handleSelectAndOpen}
        onNewChat={handleNewChat}
        onCloseTab={handleCloseTab}
        onToggleHistory={() => setChatHistoryOpen((v) => !v)}
        historyOpen={chatHistoryOpen}
        connected={connected}
        isDesktop={isDesktop}
        showNewChatTab={showNewChatTab}
        onCloseNewChatTab={() => {
          // Closing the transient "New chat" placeholder just drops it and
          // falls back to the most-recently still-open real tab (or the empty
          // new-chat state if none are open).
          setShowNewChatTab(false)
          if (!activeSessionId && openTabIds.length > 0) {
            onSelectSession(openTabIds[openTabIds.length - 1])
          }
        }}
        onExportSession={onExportSession}
      >
        <ChatHistory
          sessions={sessions}
          workspaces={workspaces}
          open={chatHistoryOpen}
          onClose={() => setChatHistoryOpen(false)}
          onCreateSession={handleNewChat}
          onSelectSession={handleSelectAndOpen}
          onRenameSession={onRenameSession}
          onExportSession={onExportSession}
          historyCapabilities={
            historyCapabilityState?.sessionId === activeSessionId
              ? historyCapabilityState.caps
              : undefined
          }
          onDeleteSession={(id) => {
            if (id === activeSessionId) onSelectSession('')
            onDeleteSession(id)
          }}
        />
      </ChatTabBar>

      {/* Disconnected banner — surfaces connection loss to the user. */}
      {!connected && (
        <Banner
          variant="warning"
          className="border-b px-3 py-2 flex items-center gap-2 shrink-0"
        >
          <WifiOff className="w-3.5 h-3.5" /> Reconnecting to daemon…
        </Banner>
      )}

      {/* Subtle retry indicator — shown while a mutating API call is backing
          off before a retry (send prompt, upload, model/profile switch).
          Kept minimal: a small spinning loader + label, not a full banner. */}
      {retrying && (
        <div className="border-b border-border px-3 py-1.5 flex items-center gap-2 shrink-0 text-xs text-muted-foreground">
          <Loader2 className="w-3 h-3 animate-spin" /> Retrying…
        </div>
      )}

      {/* MCP load/toggle failure — surfaces errors the hook used to swallow. */}
      {mcpError && (
        <Banner
          variant="error"
          className="border-b px-3 py-2 flex items-center gap-2 shrink-0"
        >
          <AlertCircle className="w-3.5 h-3.5 shrink-0" />
          <span className="flex-1">{mcpError}</span>
          <button
            type="button"
            aria-label="Dismiss"
            onClick={() => setMcpError(null)}
            className="hover:opacity-80"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </Banner>
      )}

      {/* Stuck-agent watchdog — agentRunning has stayed true with no events
          for the timeout window. Offers a manual interrupt or a wait reset. */}
      {stuckAgent && activeSessionId && (
        <Banner
          variant="warning"
          className="border-b px-3 py-2 flex items-center gap-2 shrink-0"
        >
          <AlertCircle className="w-3.5 h-3.5 shrink-0" />
          <span className="flex-1">
            Agent seems unresponsive. No activity for {stuckSeconds}s.
          </span>
          <button
            type="button"
            onClick={waitStuckAgent}
            className="hover:opacity-80 underline underline-offset-2"
          >
            Wait
          </button>
          <button
            type="button"
            onClick={handleStop}
            className="hover:opacity-80 underline underline-offset-2"
          >
            Interrupt
          </button>
        </Banner>
      )}

      <AssistantThread
        events={mergedEvents}
        pendingPermissions={pendingPermissions}
        permissionResolution={permissionResolution}
        onPermissionResponse={onPermissionResponse}
        isRunning={agentRunning}
        error={error}
        hasOlderEvents={hasOlderEvents}
        loadingOlderEvents={loadingOlderEvents}
        onLoadOlder={handleLoadOlder}
        mcpConfigChanged={mcpConfigChanged}
        onDismissMcpBanner={() => setMcpConfigChanged(false)}
        onRestartForMcp={handleRestartForMcp}
      />

      <ChatComposer
        models={currentAgent?.models ?? []}
        agentId={effectiveAgentId}
        mcpServers={mcpServers}
        mcpStatusByName={mcpHealth}
        mcpStatusLoading={mcpStatusLoading}
        onToggleMcpServer={handleToggleMcpServer}
        mcpTogglingServer={mcpTogglingServer}
        onMcpPopoutOpen={loadMcpStatus}
        onOpenMcpSettings={onOpenMcpSettings}
        effectiveModelId={effectiveModelId}
        onModelChange={handleModelChange}
        input={input}
        onInputChange={setInput}
        onSend={handleSend}
        onStop={handleStop}
        agentRunning={agentRunning}
        canSend={canSend}
        pendingPreviews={pendingPreviews}
        onRemoveAttachment={removePendingAttachment}
        onPickFiles={handlePickFiles}
        uploading={uploading}
        uploadError={uploadError}
        disabled={activeSending || agents.length === 0}
        profiles={profiles}
        selectedProfileId={selectedProfileId}
        onProfileChange={handleProfileChange}
        contextUsage={contextUsage}
      />

      <WorkspaceBar
        agents={agents}
        currentAgentId={effectiveAgentId}
        onSelectAgent={handleAgentChange}
        workspaces={workspaces}
        // Once a conversation has started, pin the display to the workspace
        // captured on the session so switching the global workspace elsewhere
        // in the app doesn't visually change the locked selector. Mirrors the
        // agent/model pinning in useAgentSelection.
        workspaceId={
          hasConversation && activeSession?.workspace
            ? activeSession.workspace
            : workspaceId
        }
        onSelectWorkspace={onSelectWorkspace}
        workspaceDisabled={hasConversation}
        disabled={activeSending || agents.length === 0}
      />

      {/* Hidden file input — triggered by the attach button via ref. */}
      <input
        ref={fileInputRef}
        type="file"
        accept="image/png,image/jpeg,image/gif,image/webp"
        multiple
        onChange={handleFileChange}
        className="hidden"
      />

      {/* Switch-agent confirmation dialog — shown when the user changes the
          harness mid-conversation. Lets them pick how much of the prior
          conversation history to transfer as context for the new agent. */}
      <SwitchAgentDialog
        open={!!pendingAgentId}
        pendingAgentId={pendingAgentId}
        currentAgentName={currentAgent?.name ?? effectiveAgentId}
        pendingAgentName={agents.find((a) => a.id === pendingAgentId)?.name ?? ''}
        truncateLength={truncateLength}
        setTruncateLength={setTruncateLength}
        onConfirm={confirmSwitchAgent}
        onCancel={cancelSwitchAgent}
        busy={rebinding}
      />
    </aside>
  )
}
