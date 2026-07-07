import { useState, useEffect, useRef, type KeyboardEvent, type ChangeEvent, type CSSProperties } from 'react'
import { Menu, Paperclip, ArrowUp, Square, Wifi, WifiOff, ChevronDown, X, Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'
import { api } from '@/lib/api'
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
import { ChatMessageItem } from './ChatMessageItem'
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

/**
 * Right sidebar — agent chat (Blueprint Sec 17 — right sidebar).
 * Contains harness/model selectors, chat history popout, conversation view
 * rendered from events, and input composer.
 *
 * On desktop: always visible alongside editor.
 * On mobile: full-screen via bottom nav.
 */
export function ChatPanel({
  events,
  agents,
  sessions,
  workspaces,
  visible,
  connected,
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
  style,
}: {
  events: AppEvent[]
  agents: Agent[]
  sessions: Session[]
  workspaces: { id: string; name: string }[]
  visible: boolean
  connected: boolean
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
      // Validate the model belongs to the selected agent (or any agent).
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
  // `pendingAgentId` holds the new agent id while the dialog is open; null
  // means the dialog is closed.
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

  // When the agents list loads asynchronously (empty on first mount), restore
  // persisted selections. Uses the "adjust state during render" pattern from
  // the React docs instead of setState-in-effect to avoid cascading renders.
  const [prevAgents, setPrevAgents] = useState(agents)
  if (agents !== prevAgents) {
    setPrevAgents(agents)
    if (agents.length > 0) {
      // Resolve the agent locally first. setState doesn't commit until after
      // this render, so reading `selectedAgent` below would see the stale
      // value — instead we track the freshly-resolved id in `nextAgent` and
      // queue the state update from it.
      let nextAgent = selectedAgent
      if (!selectedAgent || !agents.some((a) => a.id === selectedAgent)) {
        const storedAgent = localStorage.getItem('lai:selectedAgent')
        nextAgent =
          storedAgent && agents.some((a) => a.id === storedAgent)
            ? storedAgent
            : agents[0]?.id ?? ''
        if (nextAgent) setSelectedAgent(nextAgent)
      }
      // Derive the model from the freshly-resolved `nextAgent` (not the stale
      // `selectedAgent` state), so model validation runs against the correct
      // agent and doesn't pick a model from the previous selection.
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

  /** Updates models when the harness changes; rebinds an active conversation. */
  const handleAgentChange = (agentId: string) => {
    const previousAgentId = effectiveAgentId
    // If there's no active conversation, or the session has no prompts yet,
    // switch immediately without a confirmation dialog (same as before).
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
    // Mid-conversation switch — open the confirmation dialog instead of
    // rebinding immediately. The dropdown value is driven by
    // `effectiveAgentId` (derived from the active session), so it stays on
    // the current agent until the user confirms the switch.
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
    if ((!content && pendingAttachments.length === 0) || sending || !effectiveAgentId || !effectiveModelId) return

    setSending(true)
    setError(null)
    setInput('')

    try {
      let sessionId = activeSessionId
      if (!sessionId) {
        sessionId = await onCreateSession(effectiveAgentId, effectiveModelId)
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
      // A stale/deleted session surfaces a friendly message and resets to the
      // new-chat state so the user can start a fresh conversation instead of
      // retrying against a dead session id.
      if (isSessionGone(message)) {
        setError('This conversation is no longer available. Start a new chat.')
        onSelectSession('') // empty string = no active session = "new chat"
      } else {
        setError(message)
        setInput(content) // preserve the user's text so they can retry
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
    // Reset the input so picking the same file again fires change.
    e.target.value = ''

    if (!effectiveAgentId || !effectiveModelId) return
    setUploading(true)
    setUploadError(null)
    try {
      let sessionId = activeSessionId
      if (!sessionId) {
        sessionId = await onCreateSession(effectiveAgentId, effectiveModelId)
      }
      for (const file of Array.from(files)) {
        const result = await api.uploadFile(sessionId, file)
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

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  const canSend = Boolean(
    (input.trim() || pendingAttachments.length > 0) &&
      effectiveAgentId &&
      effectiveModelId &&
      !sending,
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
        // Append to the previous stream event
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
  // Depends on mergedEvents (the rendered stream) and the error banner so
  // newly surfaced errors also trigger a scroll check.
  const { isAtBottom, scrollToBottom } = useAutoscroll(
    scrollContainerRef,
    [mergedEvents, error],
  )

  const handleNewChat = () => {
    // Frontend-only: reset to a new chat state. The actual session is
    // created on the backend when the user sends their first message.
    setChatHistoryOpen(false)
    setError(null)
    setInput('')
    onSelectSession('')  // empty string = no active session = "new chat"
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
      {/* Chat Header (relative container for popout) */}
      <div className="relative border-b border-border shrink-0 bg-panel">
        {/* Top row: harness + model + hamburger */}
        <div className="flex items-center gap-2 p-3">
          {/* Harness selector — pick agent (Blueprint Sec 5) */}
          <Select value={effectiveAgentId} onValueChange={handleAgentChange} disabled={sending}>
            <SelectTrigger
              size="sm"
              className="shrink-0 text-xs font-semibold"
              title="Agent Harness"
              aria-label="Agent harness"
            >
              <SelectValue placeholder="Agent" />
            </SelectTrigger>
            <SelectContent>
              {agents.map((a) => (
                <SelectItem key={a.id} value={a.id}>{a.name}</SelectItem>
              ))}
            </SelectContent>
          </Select>

          {/* Model selector — pick model for selected agent */}
          <Select
            value={effectiveModelId}
            onValueChange={handleModelChange}
            disabled={sending || !currentAgent}
          >
            <SelectTrigger
              size="sm"
              className="shrink-0 border-primary/50 text-primary text-xs font-medium"
              title="Model"
              aria-label="Model"
            >
              <SelectValue placeholder="Model" />
            </SelectTrigger>
            <SelectContent>
              {currentAgent?.models.map((m) => (
                <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>
              ))}
            </SelectContent>
          </Select>

          <div className="flex-1" />

          {/* Connection indicator */}
          <span
            className="flex items-center gap-1 text-[11px] shrink-0"
            title={connected ? 'Connected to daemon' : 'Disconnected — reconnecting'}
          >
            {connected ? (
              <Wifi className="w-3.5 h-3.5 text-green-500" />
            ) : (
              <WifiOff className="w-3.5 h-3.5 text-red-500" />
            )}
          </span>

          {/* Hamburger menu — toggles chat list popout */}
          <button
            onClick={() => setChatHistoryOpen(!chatHistoryOpen)}
            className="p-1.5 text-muted-foreground hover:text-foreground bg-secondary hover:bg-accent rounded-md transition relative"
            title="Chat History"
            aria-label="Chat history"
            aria-expanded={chatHistoryOpen}
          >
            <Menu className="w-4 h-4" />
          </button>
        </div>

        {/* Chat list popout (floats over chat messages) */}
        <ChatHistory
          sessions={sessions}
          workspaces={workspaces}
          open={chatHistoryOpen}
          onClose={() => setChatHistoryOpen(false)}
          onCreateSession={handleNewChat}
          onSelectSession={(id) => {
            onSelectSession(id)
            setChatHistoryOpen(false)
          }}
          onRenameSession={onRenameSession}
          onExportSession={onExportSession}
          onDeleteSession={(id) => {
            if (id === activeSessionId) onSelectSession('')
            onDeleteSession(id)
          }}
        />
      </div>

      {/* Disconnected banner — surfaces connection loss to the user */}
      {!connected && (
        <div className="bg-amber-950/40 border-b border-amber-500/40 px-3 py-2 text-xs text-amber-300 flex items-center gap-2 shrink-0">
          <WifiOff className="w-3.5 h-3.5" /> Reconnecting to daemon…
        </div>
      )}

      {/* Chat Messages — rendered from event stream (Blueprint Sec 11) */}
      <div className="relative flex-1 min-h-0">
        <div
          ref={scrollContainerRef}
          className="h-full overflow-y-auto p-3 lg:p-4 space-y-3 lg:space-y-4 pb-20 lg:pb-4"
        >
          {mergedEvents.length === 0 && (
            <div className="rounded-lg border border-border bg-panel/50 p-3 text-xs text-muted-foreground">
              Send a message to start a conversation.
            </div>
          )}
          {mergedEvents.map((event, i) => (
            <ChatMessageItem
              key={event.id ?? `${event.type}-${i}`}
              event={event}
              pending={
                event.type === 'PermissionRequested' && event.requestId
                  ? pendingPermissions.find((p) => p.id === event.requestId)
                  : undefined
              }
              resolution={
                event.type === 'PermissionRequested' && event.requestId
                  ? permissionResolution.get(event.requestId)
                  : undefined
              }
              onPermissionResponse={onPermissionResponse}
            />
          ))}
          {error && (
            <div className="rounded-lg border border-red-500/40 bg-red-950/20 p-3 text-xs text-red-300">
              {error}
            </div>
          )}
        </div>

        {/* Jump-to-bottom button — shown only when the user has scrolled
            away from the bottom (Feature 1). Clicking snaps to the bottom. */}
        {!isAtBottom && (
          <button
            onClick={scrollToBottom}
            className="absolute bottom-4 right-4 rounded-full bg-background border border-border p-2 shadow-md hover:bg-accent text-muted-foreground hover:text-foreground transition"
            title="Jump to bottom"
            aria-label="Jump to bottom"
          >
            <ChevronDown className="w-4 h-4" />
          </button>
        )}
      </div>

      {/* Chat Input (Blueprint Sec 17 — input composer) */}
      <div className="p-2.5 lg:p-3 bg-gradient-to-t from-background to-transparent shrink-0 border-t border-border/50 pb-20 lg:pb-3">
        {/* Pending attachment previews — shown above the textarea while
            composing. Each chip shows a thumbnail + filename + remove button. */}
        {pendingPreviews.length > 0 && (
          <div className="flex flex-wrap gap-2 mb-2">
            {pendingPreviews.map((preview, i) => (
              <div
                key={`${preview.url}-${i}`}
                className="relative group flex items-center gap-2 rounded-lg border border-border bg-muted px-2 py-1.5 pr-7 max-w-[180px]"
              >
                <img
                  src={preview.url}
                  alt={preview.name}
                  className="w-8 h-8 rounded object-cover shrink-0 border border-border"
                />
                <span className="text-xs text-muted-foreground truncate" title={preview.name}>
                  {preview.name}
                </span>
                <button
                  onClick={() => removePendingAttachment(i)}
                  className="absolute right-1 top-1/2 -translate-y-1/2 p-0.5 rounded text-muted-foreground hover:text-foreground hover:bg-accent transition"
                  title="Remove attachment"
                  aria-label={`Remove ${preview.name}`}
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            ))}
          </div>
        )}
        {uploadError && (
          <div className="mb-2 rounded-md border border-red-500/40 bg-red-950/20 px-2.5 py-1.5 text-xs text-red-300">
            {uploadError}
          </div>
        )}
        <div className="relative flex items-end gap-2">
          <button
            onClick={handlePickFiles}
            disabled={uploading || sending || agents.length === 0}
            className="p-2.5 bg-panel border border-border rounded-xl hover:bg-accent hover:border-ring transition text-muted-foreground hover:text-foreground shrink-0 disabled:opacity-60 disabled:cursor-not-allowed"
            title="Upload image"
            aria-label="Upload image"
          >
            {uploading ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Paperclip className="w-4 h-4" />
            )}
          </button>
          <input
            ref={fileInputRef}
            type="file"
            accept="image/png,image/jpeg,image/gif,image/webp"
            multiple
            onChange={handleFileChange}
            className="hidden"
          />
          <div className="relative flex-1">
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={agents.length === 0 ? 'Configure an agent first...' : 'Message agent...'}
              disabled={sending || agents.length === 0}
              className="w-full bg-panel border border-input rounded-xl pl-3 pr-10 py-3 text-sm focus:outline-none focus:border-ring focus:ring-1 focus:ring-ring resize-none h-12 disabled:opacity-60 disabled:cursor-not-allowed transition-all"
            />
            {agentRunning && activeSessionId ? (
              <button
                onClick={handleStop}
                className="absolute right-2 bottom-2 p-1.5 bg-red-600 rounded-lg hover:bg-red-500 transition"
                title="Stop"
                aria-label="Stop"
              >
                <Square className="w-3.5 h-3.5 text-white" />
              </button>
            ) : (
              <button
                onClick={handleSend}
                disabled={!canSend}
                className="absolute right-2 bottom-2 p-1.5 bg-primary rounded-lg hover:bg-primary/90 disabled:bg-muted disabled:cursor-not-allowed transition"
                aria-label="Send message"
              >
                <ArrowUp className="w-3.5 h-3.5 text-primary-foreground" />
              </button>
            )}
          </div>
        </div>
      </div>

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
