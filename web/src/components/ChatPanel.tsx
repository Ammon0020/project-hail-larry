import { useState, useRef, useMemo, useEffect, type ChangeEvent, type CSSProperties } from 'react'
import { WifiOff } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { UploadResult } from '@/lib/api'
import { getMcpConfig, patchMcpServer } from '@/lib/api'
import { ChatTabBar } from './ChatTabBar'
import { ChatComposer } from './ChatComposer'
import { ConversationView } from './ConversationView'
import { ChatHistory } from './ChatHistory'
import { SwitchAgentDialog } from './SwitchAgentDialog'
import { WorkspaceBar } from './chat/WorkspaceBar'
import { useAutoscroll } from '@/hooks/useAutoscroll'
import { useLocalStorage } from '@/hooks/useLocalStorage'
import type { AppEvent, Agent, Attachment, Session } from '@/types'
import type { PendingPermission } from '@/lib/api'

/**
 * Returns true when an error message indicates the active conversation no
 * longer exists in the backend (e.g. "session not found" or the friendly
 * "no longer available" message thrown by useBackend.sendPrompt). Used to
 * reset the UI to the new-chat state instead of showing a raw error.
 */
function isSessionGone(message: string): boolean {
  const lower = message.toLowerCase()
  return (
    lower.includes('session not found') ||
    lower.includes('no longer available') ||
    lower.includes('not found')
  )
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
  onUploadFile,
  workspaceId,
  onSelectWorkspace,
  style,
}: {
  events: AppEvent[]
  /** All events across all sessions — used only to compute the running
   *  indicator for non-active open tabs. The active session's running state
   *  is derived from `events` (already filtered to it) plus the `sending`
   *  state. Don't use this for conversation rendering. */
  allEvents: AppEvent[]
  agents: Agent[]
  sessions: Session[]
  workspaces: { id: string; name: string }[]
  visible: boolean
  connected: boolean
  isDesktop: boolean
  pendingPermissions: PendingPermission[]
  activeSessionId: string | null
  onSendMessage: (sessionId: string, content: string, attachments?: Attachment[]) => Promise<void>
  onCreateSession: (agentId: string, modelId: string) => Promise<string>
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
  /** Uploads a file to a session's upload store. Routed through useBackend so
   *  uploads share the hook's session-recovery semantics instead of bypassing
   *  it via api.uploadFile directly. */
  onUploadFile: (sessionId: string, file: File) => Promise<UploadResult>
  /** Active workspace id. */
  workspaceId: string
  /** Workspace change handler. */
  onSelectWorkspace: (id: string) => void
  /** Optional inline style — used by App.tsx to apply a persisted panel width on desktop. */
  style?: CSSProperties
}) {
  // Persisted agent/model selections and open-tab ids — restored from
  // localStorage on mount via useLocalStorage, falling back to the first
  // available agent/model when the stored value is missing or no longer valid
  // (UI Spec §6.2 — UI Persistence). The selected* values below are derived
  // from the stored values against the current agents list so a stale stored
  // selection (agent/model removed since last run) is corrected reactively as
  // agents load asynchronously — no separate prevAgents reconciliation block.
  const [storedAgent, setStoredAgent] = useLocalStorage<string>('lai:selectedAgent', '')
  const [storedModel, setStoredModel] = useLocalStorage<string>('lai:selectedModel', '')
  const [openTabIds, setOpenTabIds] = useLocalStorage<string[]>('lai:openTabIds', [])
  const selectedAgent = agents.some((a) => a.id === storedAgent)
    ? storedAgent
    : (agents[0]?.id ?? '')
  const agentForModel = agents.find((a) => a.id === selectedAgent)
  const selectedModel = agentForModel?.models.some((m) => m.id === storedModel)
    ? storedModel
    : (agentForModel?.models[0]?.id ?? '')

  const [chatHistoryOpen, setChatHistoryOpen] = useState(false)
  const [input, setInput] = useState('')
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)

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

  // Switch-agent confirmation dialog state. When the user changes the agent
  // dropdown mid-conversation, we show a dialog instead of rebinding
  // immediately so they can pick a transfer-history truncate length.
  const [pendingAgentId, setPendingAgentId] = useState<string | null>(null)
  const [truncateLength, setTruncateLength] = useState<number>(8000)

  // MCP server list + toggle state for the chat overflow menu's "MCP servers"
  // sub-menu. The settings panel is the primary editing surface; the chat menu
  // just reflects the current config and lets users toggle servers inline.
  // ACP mandates a restart to apply MCP changes (no live add/remove in v1), so
  // toggling a server while a session is active surfaces a restart banner.
  const [mcpServers, setMcpServers] = useState<{ name: string; enabled: boolean }[]>([])
  const [mcpTogglingServer, setMcpTogglingServer] = useState<string | null>(null)
  const [mcpConfigChanged, setMcpConfigChanged] = useState(false)

  async function loadMcpServers() {
    try {
      const text = await getMcpConfig()
      const parsed = JSON.parse(text) as { mcpServers?: Record<string, { enabled?: boolean }> }
      const entries = Object.entries(parsed.mcpServers || {})
      setMcpServers(
        entries.map(([name, cfg]) => ({
          name,
          enabled: cfg.enabled !== false,
        })),
      )
    } catch {
      // If MCP config can't be loaded, just show empty list
    }
  }

  useEffect(() => {
    // loadMcpServers is async: setState runs after `await getMcpConfig()`,
    // so it's not synchronous within this effect body. The disable is needed
    // because the linter can't see through the function boundary.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadMcpServers()
  }, [])

  const handleToggleMcpServer = async (name: string, enabled: boolean) => {
    setMcpTogglingServer(name)
    try {
      await patchMcpServer(name, enabled)
      await loadMcpServers()
      setMcpConfigChanged(true)
    } catch (e) {
      console.error('Failed to toggle MCP server:', e)
    } finally {
      setMcpTogglingServer(null)
    }
  }

  /** Restart for MCP: ACP doesn't support live add/remove in v1, so the
   *  simplest correct action is to drop the active session and let the user
   *  start a new chat, which will pick up the updated MCP config. */
  const handleRestartForMcp = () => {
    setMcpConfigChanged(false)
    onSelectSession('')
    setShowNewChatTab(true)
  }

  // Scroll container ref for the smart-autoscroll hook (Feature 1).
  const scrollContainerRef = useRef<HTMLDivElement>(null)

  // Auto-open the active session as a tab. New chats (created via handleSend /
  // handleFileChange) and existing chats restored via activeSessionId (e.g.
  // cold reload, history click that didn't go through handleSelectAndOpen)
  // would otherwise never enter openTabIds, so they wouldn't render a tab.
  // Uses the "adjust state during render" pattern (React docs) — same as the
  // prevSessions block below — to avoid react-hooks/set-state-in-effect.
  const [prevActiveSessionId, setPrevActiveSessionId] = useState(activeSessionId)
  if (activeSessionId !== prevActiveSessionId) {
    setPrevActiveSessionId(activeSessionId)
    // Only auto-open a real session as a tab. The new-chat state
    // (activeSessionId is null/empty) is represented by the transient
    // `showNewChatTab` placeholder, NOT an entry in openTabIds — adding an
    // empty id here would corrupt the persisted openTabIds list.
    if (activeSessionId && sessions.some((s) => s.id === activeSessionId)) {
      setOpenTabIds((prev) =>
        prev.includes(activeSessionId) ? prev : [...prev, activeSessionId],
      )
    }
  }

  // Drop open-tab ids whose sessions have been deleted (e.g. after a daemon
  // restart that wiped conversations.json). Render-time pure adjustment.
  // Also auto-opens the active session as a tab — covers the cold-reload case
  // where activeSessionId is restored from localStorage immediately but
  // sessions load asynchronously, so the prevActiveSessionId block above
  // (which only fires on activeSessionId change) would miss it.
  const [prevSessions, setPrevSessions] = useState(sessions)
  if (sessions !== prevSessions) {
    setPrevSessions(sessions)
    const known = new Set(sessions.map((s) => s.id))
    setOpenTabIds((prev) => {
      const filtered = prev.filter((id) => known.has(id))
      if (activeSessionId && known.has(activeSessionId) && !filtered.includes(activeSessionId)) {
        return [...filtered, activeSessionId]
      }
      return filtered
    })
  }

  // The active conversation owns its agent/model — derive the selectors from it
  // so switching reflects that conversation. For a new chat (no active session)
  // the local selection drives the choice. The locally-selected model takes
  // priority over the session's persisted model so the dropdown visually updates
  // immediately when the user picks a different model — the backend switch is
  // deferred to send time (see handleSend). The guard `currentAgent?.models.some`
  // ensures the stored model belongs to the effective agent, so switching to a
  // session with a different agent doesn't show a stale model from the old one.
  const activeSession = sessions.find((s) => s.id === activeSessionId)
  const effectiveAgentId = activeSession?.agentId || selectedAgent || agents[0]?.id || ''
  const currentAgent = agents.find((a) => a.id === effectiveAgentId)
  const userSelectedModel =
    selectedModel && currentAgent?.models.some((m) => m.id === selectedModel)
      ? selectedModel
      : null
  const effectiveModelId =
    userSelectedModel || activeSession?.modelId || currentAgent?.models[0]?.id || ''

  // Determine whether a turn is currently in flight, to toggle Send/Stop.
  const lastEvent = events[events.length - 1]
  const agentRunning =
    sending ||
    (!!lastEvent &&
      ((lastEvent.type === 'StreamUpdate' && !!lastEvent.streaming) ||
        lastEvent.type === 'ResponseStarted' ||
        lastEvent.type === 'ToolStarted' ||
        lastEvent.type === 'ShellCommandStarted'))

  // Per-tab running indicator. The active session uses the global `sending`
  // state (only one send can be in flight at a time and it always targets the
  // active session) plus its last event. Non-active open tabs are derived
  // purely from their last event in `allEvents` — `sending` is NOT applied to
  // them, since a background tab's send state is owned by whichever session
  // was active when it was dispatched.
  const runningSessionIds = useMemo(() => {
    const ids = new Set<string>()
    const isRunningEvent = (e: AppEvent | undefined): boolean =>
      !!e &&
      ((e.type === 'StreamUpdate' && !!e.streaming) ||
        e.type === 'ResponseStarted' ||
        e.type === 'ToolStarted' ||
        e.type === 'ShellCommandStarted')
    if (activeSessionId && (sending || isRunningEvent(lastEvent))) {
      ids.add(activeSessionId)
    }
    for (const id of openTabIds) {
      if (id === activeSessionId) continue
      const eventsForTab = allEvents.filter((e) => e.sessionId === id)
      if (isRunningEvent(eventsForTab[eventsForTab.length - 1])) {
        ids.add(id)
      }
    }
    return ids
  }, [activeSessionId, sending, lastEvent, openTabIds, allEvents])

  // Tabs to render — sessions whose id is in openTabIds, in openTabIds order.
  const openTabs = openTabIds
    .map((id) => sessions.find((s) => s.id === id))
    .filter((s): s is Session => !!s)

  /** Adds a session id to the open tabs (idempotent — no reorder if already open). */
  const openTab = (id: string) => {
    setOpenTabIds((prev) => (prev.includes(id) ? prev : [...prev, id]))
  }

  /** Hides a tab without deleting the session. If the closed tab was active,
   *  fall back to the most-recently still-open tab (or new-chat state). */
  const handleCloseTab = (id: string) => {
    setOpenTabIds((prev) => {
      const next = prev.filter((tid) => tid !== id)
      if (id === activeSessionId) {
        if (next.length > 0) {
          onSelectSession(next[next.length - 1])
        } else {
          onSelectSession('')
        }
      }
      return next
    })
  }

  const hasConversation =
    !!activeSessionId && events.some((e) => e.type === 'PromptSubmitted')

  /** Updates models when the harness changes; rebinds an active conversation. */
  const handleAgentChange = (agentId: string) => {
    const previousAgentId = effectiveAgentId
    if (!hasConversation) {
      setStoredAgent(agentId)
      const agent = agents.find((a) => a.id === agentId)
      const modelId = agent?.models[0]?.id ?? ''
      if (agent) setStoredModel(modelId)
      if (activeSessionId && modelId) onRebindSession(activeSessionId, agentId, modelId)
      return
    }
    if (agentId === previousAgentId) return
    setPendingAgentId(agentId)
  }

  /** Confirms the pending agent switch and rebinds with the chosen truncate length. */
  const confirmSwitchAgent = () => {
    if (!pendingAgentId || !activeSessionId) {
      setPendingAgentId(null)
      return
    }
    const agentId = pendingAgentId
    setStoredAgent(agentId)
    const agent = agents.find((a) => a.id === agentId)
    const modelId = agent?.models[0]?.id ?? ''
    if (agent) setStoredModel(modelId)
    const maxBytes = truncateLength > 0 ? truncateLength : undefined
    onRebindSession(activeSessionId, agentId, modelId, maxBytes)
    setPendingAgentId(null)
  }

  /** Cancels the pending agent switch — reverts the dropdown to the current agent. */
  const cancelSwitchAgent = () => {
    setPendingAgentId(null)
  }

  /** Updates the locally-selected model only. The backend switch is deferred
   *  to send time (see handleSend) so the user can change their mind in the
   *  dropdown without round-tripping to the server on every selection — only
   *  the model that's actually in effect when a prompt is sent gets applied. */
  const handleModelChange = (modelId: string) => {
    setStoredModel(modelId)
  }

  const handleSend = async () => {
    const content = input.trim()
    if ((!content && pendingAttachments.length === 0) || sending || uploading || !effectiveAgentId || !effectiveModelId) return

    setSending(true)
    setError(null)
    setInput('')

    try {
      let sessionId = activeSessionId
      const wasNewSession = !sessionId
      if (!sessionId) {
        sessionId = await onCreateSession(effectiveAgentId, effectiveModelId)
        // A real session now exists — drop the transient "New chat" placeholder.
        setShowNewChatTab(false)
      }
      // Always ensure the active session is opened as a tab — covers both the
      // newly-created case above and an existing activeSessionId that hasn't
      // been added to openTabIds yet (the auto-open effect also handles this,
      // but calling it here makes the tab appear immediately on send).
      openTab(sessionId)
      // Deferred model switch: handleModelChange only updates local state, so
      // if the user picked a model that differs from an existing session's
      // current model, apply the switch on the backend now so this prompt uses
      // it. Skipped for newly-created sessions — onCreateSession already used
      // the selected model, so switching again would be redundant.
      if (!wasNewSession && userSelectedModel && activeSession?.modelId !== userSelectedModel) {
        await onSwitchModel(sessionId, userSelectedModel)
      }
      const attachmentsToSend = pendingAttachments
      await onSendMessage(
        sessionId,
        content,
        attachmentsToSend.length > 0 ? attachmentsToSend : undefined,
      )
      setPendingAttachments([])
      setPendingPreviews([])
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to send message'
      if (isSessionGone(message)) {
        setError('This conversation is no longer available. Start a new chat.')
        onSelectSession('')
      } else {
        setError(message)
        setInput(content)
      }
    } finally {
      setSending(false)
    }
  }

  const handleStop = () => {
    if (activeSessionId) onCancel(activeSessionId)
    setSending(false)
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
        sessionId = await onCreateSession(effectiveAgentId, effectiveModelId)
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
      if (isSessionGone(message)) {
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
  const permissionResolution = new Map<string, 'granted' | 'denied'>()
  for (const e of events) {
    if (e.requestId && (e.type === 'PermissionGranted' || e.type === 'PermissionDenied')) {
      permissionResolution.set(e.requestId, e.type === 'PermissionDenied' ? 'denied' : 'granted')
    }
  }

  const canSend = Boolean(
    (input.trim() || pendingAttachments.length > 0) &&
      effectiveAgentId &&
      effectiveModelId &&
      !sending &&
      !uploading,
  )

  /**
   * Merges consecutive StreamUpdate events into a single accumulated message
   * so streaming text appears as one growing response (like ChatGPT).
   * Non-stream events are passed through individually.
   */
  const mergedEvents: AppEvent[] = events.reduce((acc: AppEvent[], event: AppEvent) => {
    if (event.type === 'StreamUpdate') {
      const last = acc[acc.length - 1]
      if (last && last.type === 'StreamUpdate' && last.role === event.role && !!last.thought === !!event.thought) {
        acc[acc.length - 1] = {
          ...last,
          content: (last.content || '') + (event.content || ''),
          streaming: event.streaming,
        }
        return acc
      }
    }
    acc.push(event)
    return acc
  }, [])

  // Smart autoscroll — follows new content only when the user is already
  // near the bottom; otherwise stays put and shows a jump-to-bottom button.
  // `pendingPermissions` is in the deps because it arrives via a separate
  // async REST call (loadPendingPermissions) after the PermissionRequested
  // event — when it lands the permission card grows to show action buttons,
  // and we need to scroll again so the card isn't cut off at the bottom.
  const { isAtBottom, scrollToBottom } = useAutoscroll(
    scrollContainerRef,
    [mergedEvents, error, pendingPermissions],
  )

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
          onDeleteSession={(id) => {
            if (id === activeSessionId) onSelectSession('')
            onDeleteSession(id)
          }}
        />
      </ChatTabBar>

      {/* Disconnected banner — surfaces connection loss to the user. */}
      {!connected && (
        <div className="bg-warning/10 border-b border-warning/40 px-3 py-2 text-xs text-warning flex items-center gap-2 shrink-0">
          <WifiOff className="w-3.5 h-3.5" /> Reconnecting to daemon…
        </div>
      )}

      <ConversationView
        events={mergedEvents}
        pendingPermissions={pendingPermissions}
        permissionResolution={permissionResolution}
        onPermissionResponse={onPermissionResponse}
        error={error}
        scrollContainerRef={scrollContainerRef}
        isAtBottom={isAtBottom}
        onJumpToBottom={scrollToBottom}
        mcpConfigChanged={mcpConfigChanged}
        onDismissMcpBanner={() => setMcpConfigChanged(false)}
        onRestartForMcp={handleRestartForMcp}
      />

      <ChatComposer
        models={currentAgent?.models ?? []}
        mcpServers={mcpServers}
        onToggleMcpServer={handleToggleMcpServer}
        mcpTogglingServer={mcpTogglingServer}
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
        disabled={sending || agents.length === 0}
      />

      <WorkspaceBar
        agents={agents}
        currentAgentId={effectiveAgentId}
        onSelectAgent={handleAgentChange}
        workspaces={workspaces}
        workspaceId={workspaceId}
        onSelectWorkspace={onSelectWorkspace}
        workspaceDisabled={hasConversation}
        disabled={sending || agents.length === 0}
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
      />
    </aside>
  )
}
