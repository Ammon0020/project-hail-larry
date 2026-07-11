import { useState, useEffect, useRef, useMemo, type ChangeEvent, type CSSProperties } from 'react'
import { WifiOff } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { UploadResult } from '@/lib/api'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { ChatTabBar } from './ChatTabBar'
import { ChatComposer } from './ChatComposer'
import { ConversationView } from './ConversationView'
import { ChatHistory } from './ChatHistory'
import { useAutoscroll } from '@/hooks/useAutoscroll'
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

/** Reads persisted open-tab ids from localStorage, validating each against the
 *  known session ids. Stale ids (deleted sessions) are dropped silently. */
function loadOpenTabIds(knownIds: Set<string>): string[] {
  try {
    const stored = localStorage.getItem('lai:openTabIds')
    if (!stored) return []
    const parsed = JSON.parse(stored)
    if (!Array.isArray(parsed)) return []
    return parsed.filter((id): id is string => typeof id === 'string' && knownIds.has(id))
  } catch {
    return []
  }
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
  onExportSession,
  onUploadFile,
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
  onExportSession: (sessionId: string) => void
  /** Uploads a file to a session's upload store. Routed through useBackend so
   *  uploads share the hook's session-recovery semantics instead of bypassing
   *  it via api.uploadFile directly. */
  onUploadFile: (sessionId: string, file: File) => Promise<UploadResult>
  /** Optional inline style — used by App.tsx to apply a persisted panel width on desktop. */
  style?: CSSProperties
}) {
  // Persisted agent/model selections — restored from localStorage on mount,
  // falling back to the first available agent/model if the stored value is
  // missing or no longer valid (UI Spec §6.2 — UI Persistence).
  const [selectedAgent, setSelectedAgent] = useState(() => {
    const stored = localStorage.getItem('lai:selectedAgent')
    if (stored && agents.some((a) => a.id === stored)) return stored
    return agents[0]?.id ?? ''
  })
  const [selectedModel, setSelectedModel] = useState(() => {
    const stored = localStorage.getItem('lai:selectedModel')
    if (stored) {
      const agent = agents.find((a) => a.id === selectedAgent)
      if (agent?.models.some((m) => m.id === stored)) return stored
      if (agents.some((a) => a.models.some((m) => m.id === stored))) return stored
    }
    return agents[0]?.models[0]?.id ?? ''
  })
  const [chatHistoryOpen, setChatHistoryOpen] = useState(false)
  const [input, setInput] = useState('')
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Open-tab ids — local UI state for which sessions appear as tabs. Closing
  // a tab hides it (does NOT delete the session); the agent keeps running in
  // the background. Persisted to localStorage so open tabs survive a reload.
  const [openTabIds, setOpenTabIds] = useState<string[]>(() =>
    loadOpenTabIds(new Set(sessions.map((s) => s.id))),
  )

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

  // Scroll container ref for the smart-autoscroll hook (Feature 1).
  const scrollContainerRef = useRef<HTMLDivElement>(null)

  // Persist agent and model selections whenever they change.
  useEffect(() => {
    if (selectedAgent) localStorage.setItem('lai:selectedAgent', selectedAgent)
  }, [selectedAgent])

  useEffect(() => {
    if (selectedModel) localStorage.setItem('lai:selectedModel', selectedModel)
  }, [selectedModel])

  // Persist open-tab ids.
  useEffect(() => {
    localStorage.setItem('lai:openTabIds', JSON.stringify(openTabIds))
  }, [openTabIds])

  // Auto-open the active session as a tab. New chats (created via handleSend /
  // handleFileChange) and existing chats restored via activeSessionId (e.g.
  // cold reload, history click that didn't go through handleSelectAndOpen)
  // would otherwise never enter openTabIds, so they wouldn't render a tab.
  // Uses the "adjust state during render" pattern (React docs) — same as the
  // prevSessions block below — to avoid react-hooks/set-state-in-effect.
  const [prevActiveSessionId, setPrevActiveSessionId] = useState(activeSessionId)
  if (activeSessionId !== prevActiveSessionId) {
    setPrevActiveSessionId(activeSessionId)
    if (activeSessionId && sessions.some((s) => s.id === activeSessionId)) {
      setOpenTabIds((prev) =>
        prev.includes(activeSessionId) ? prev : [...prev, activeSessionId],
      )
    }
  }

  // When the agents list loads asynchronously (empty on first mount), restore
  // persisted selections. Uses the "adjust state during render" pattern from
  // the React docs instead of setState-in-effect to avoid cascading renders.
  const [prevAgents, setPrevAgents] = useState(agents)
  if (agents !== prevAgents) {
    setPrevAgents(agents)
    if (agents.length > 0) {
      let nextAgent = selectedAgent
      if (!selectedAgent || !agents.some((a) => a.id === selectedAgent)) {
        const storedAgent = localStorage.getItem('lai:selectedAgent')
        nextAgent =
          storedAgent && agents.some((a) => a.id === storedAgent)
            ? storedAgent
            : agents[0]?.id ?? ''
        if (nextAgent) setSelectedAgent(nextAgent)
      }
      const agent = agents.find((a) => a.id === nextAgent)
      if (!selectedModel || !agent?.models.some((m) => m.id === selectedModel)) {
        const storedModel = localStorage.getItem('lai:selectedModel')
        const validModel =
          storedModel && agent?.models.some((m) => m.id === storedModel)
            ? storedModel
            : agent?.models[0]?.id ?? ''
        if (validModel) setSelectedModel(validModel)
      }
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
  // the local selection drives the choice.
  const activeSession = sessions.find((s) => s.id === activeSessionId)
  const effectiveAgentId = activeSession?.agentId || selectedAgent || agents[0]?.id || ''
  const currentAgent = agents.find((a) => a.id === effectiveAgentId)
  const effectiveModelId =
    activeSession?.modelId || selectedModel || currentAgent?.models[0]?.id || ''

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

  /** Updates models when the harness changes; rebinds an active conversation. */
  const handleAgentChange = (agentId: string) => {
    const previousAgentId = effectiveAgentId
    const hasConversation =
      !!activeSessionId && events.some((e) => e.type === 'PromptSubmitted')
    if (!hasConversation) {
      setSelectedAgent(agentId)
      const agent = agents.find((a) => a.id === agentId)
      const modelId = agent?.models[0]?.id ?? ''
      if (agent) setSelectedModel(modelId)
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
    setSelectedAgent(agentId)
    const agent = agents.find((a) => a.id === agentId)
    const modelId = agent?.models[0]?.id ?? ''
    if (agent) setSelectedModel(modelId)
    const maxBytes = truncateLength > 0 ? truncateLength : undefined
    onRebindSession(activeSessionId, agentId, modelId, maxBytes)
    setPendingAgentId(null)
  }

  /** Cancels the pending agent switch — reverts the dropdown to the current agent. */
  const cancelSwitchAgent = () => {
    setPendingAgentId(null)
  }

  /** Switches model; rebinds an active conversation in place. */
  const handleModelChange = (modelId: string) => {
    setSelectedModel(modelId)
    if (activeSessionId) onRebindSession(activeSessionId, effectiveAgentId, modelId)
  }

  const handleSend = async () => {
    const content = input.trim()
    if ((!content && pendingAttachments.length === 0) || sending || uploading || !effectiveAgentId || !effectiveModelId) return

    setSending(true)
    setError(null)
    setInput('')

    try {
      let sessionId = activeSessionId
      if (!sessionId) {
        sessionId = await onCreateSession(effectiveAgentId, effectiveModelId)
      }
      // Always ensure the active session is open as a tab — covers both the
      // newly-created case above and an existing activeSessionId that hasn't
      // been added to openTabIds yet (the auto-open effect also handles this,
      // but calling it here makes the tab appear immediately on send).
      openTab(sessionId)
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
      }
      // Always ensure the active session is open as a tab (see handleSend).
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
  const { isAtBottom, scrollToBottom } = useAutoscroll(
    scrollContainerRef,
    [mergedEvents, error],
  )

  const handleNewChat = async () => {
    // Reset transient UI state, then create a fresh session on the backend so
    // it shows up as a tab immediately. The auto-open effect adds the new
    // session's id to openTabIds once activeSessionId propagates back from
    // App. If no agent/model is selectable we fall back to the empty-string
    // selection (new-chat placeholder state) — there's nothing to create.
    setChatHistoryOpen(false)
    setError(null)
    setInput('')
    if (effectiveAgentId && effectiveModelId) {
      try {
        await onCreateSession(effectiveAgentId, effectiveModelId)
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to start new chat')
      }
    } else {
      onSelectSession('')
    }
  }

  /** Selecting a session from a tab or the history popout also opens it as a tab. */
  const handleSelectAndOpen = (id: string) => {
    if (id) openTab(id)
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
      />

      <ChatComposer
        agents={agents}
        effectiveAgentId={effectiveAgentId}
        effectiveModelId={effectiveModelId}
        onAgentChange={handleAgentChange}
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
      <Dialog
        open={!!pendingAgentId}
        onOpenChange={(o) => {
          if (!o) cancelSwitchAgent()
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Switch Agent</DialogTitle>
            <DialogDescription>
              Switching from{' '}
              <span className="font-semibold text-foreground">
                {currentAgent?.name ?? effectiveAgentId}
              </span>{' '}
              to{' '}
              <span className="font-semibold text-foreground">
                {agents.find((a) => a.id === pendingAgentId)?.name ?? pendingAgentId}
              </span>{' '}
              will start a fresh conversation. The previous conversation history will be
              transferred as context (truncated to{' '}
              <span className="font-semibold text-foreground">
                {truncateLength > 0
                  ? `${truncateLength.toLocaleString()} chars`
                  : 'no limit'}
              </span>
              ).
            </DialogDescription>
          </DialogHeader>

          {/* Truncate length control */}
          <div className="space-y-2">
            <label
              htmlFor="truncate-length"
              className="block text-xs font-medium text-muted-foreground"
            >
              Transfer history length
            </label>
            <Select
              value={String(truncateLength)}
              onValueChange={(v) => setTruncateLength(Number(v))}
            >
              <SelectTrigger
                id="truncate-length"
                className="w-full"
                aria-label="Transfer history length"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="4000">4,000 chars</SelectItem>
                <SelectItem value="8000">8,000 chars</SelectItem>
                <SelectItem value="16000">16,000 chars</SelectItem>
                <SelectItem value="32000">32,000 chars</SelectItem>
                <SelectItem value="0">Full (no limit)</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <DialogFooter>
            <Button variant="secondary" onClick={cancelSwitchAgent}>
              Cancel
            </Button>
            <Button onClick={confirmSwitchAgent}>Switch Agent</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </aside>
  )
}
