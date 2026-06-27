import { useState, type KeyboardEvent } from 'react'
import { Menu, Paperclip, ArrowUp, Square, Wifi, WifiOff } from 'lucide-react'
import { cn } from '@/lib/utils'
import { ChatMessageItem } from './ChatMessageItem'
import { ChatHistory } from './ChatHistory'
import type { AppEvent, Agent, Session } from '@/types'
import type { PendingPermission } from '@/lib/api'

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
}: {
  events: AppEvent[]
  agents: Agent[]
  sessions: Session[]
  visible: boolean
  connected: boolean
  pendingPermissions: PendingPermission[]
  activeSessionId: string | null
  onSendMessage: (sessionId: string, content: string) => Promise<void>
  onCreateSession: (agentId: string, modelId: string) => Promise<string>
  onPermissionResponse: (requestId: string, decision: string) => void
  onSelectSession: (sessionId: string) => void
  onCancel: (sessionId: string) => void
  onRenameSession: (sessionId: string, name: string) => void
  onDeleteSession: (sessionId: string) => void
  onRebindSession: (sessionId: string, agentId: string, modelId: string) => void
  onExportSession: (sessionId: string) => void
}) {
  const [selectedAgent, setSelectedAgent] = useState(agents[0]?.id ?? '')
  const [selectedModel, setSelectedModel] = useState(agents[0]?.models[0]?.id ?? '')
  const [chatHistoryOpen, setChatHistoryOpen] = useState(false)
  const [input, setInput] = useState('')
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)

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
    setSelectedAgent(agentId)
    const agent = agents.find((a) => a.id === agentId)
    const modelId = agent?.models[0]?.id ?? ''
    if (agent) setSelectedModel(modelId)
    if (activeSessionId && modelId) onRebindSession(activeSessionId, agentId, modelId)
  }

  /** Switches model; rebinds an active conversation in place. */
  const handleModelChange = (modelId: string) => {
    setSelectedModel(modelId)
    if (activeSessionId) onRebindSession(activeSessionId, effectiveAgentId, modelId)
  }

  const handleSend = async () => {
    const content = input.trim()
    if (!content || sending || !effectiveAgentId || !effectiveModelId) return

    setSending(true)
    setError(null)
    setInput('')

    try {
      let sessionId = activeSessionId
      if (!sessionId) {
        sessionId = await onCreateSession(effectiveAgentId, effectiveModelId)
      }
      await onSendMessage(sessionId, content)
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to send message'
      setError(message)
      setInput(content) // preserve the user's text so they can retry
    } finally {
      setSending(false)
    }
  }

  const handleStop = () => {
    if (activeSessionId) onCancel(activeSessionId)
    setSending(false)
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

  const canSend = Boolean(input.trim() && effectiveAgentId && effectiveModelId && !sending)

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
        'flex-col h-full shrink-0 w-full bg-background border-l border-gray-800 lg:w-96',
        visible ? 'flex' : 'hidden',
        'absolute inset-0 z-30 lg:relative lg:inset-auto lg:z-auto',
      )}
    >
      {/* Chat Header (relative container for popout) */}
      <div className="relative border-b border-gray-800 shrink-0 bg-panel">
        {/* Top row: harness + model + hamburger */}
        <div className="flex items-center gap-2 p-3">
          {/* Harness selector — pick agent (Blueprint Sec 5) */}
          <select
            value={effectiveAgentId}
            onChange={(e) => handleAgentChange(e.target.value)}
            disabled={sending}
            className="appearance-none bg-background border border-gray-700 text-gray-200 text-xs font-semibold rounded-md py-1.5 pl-2.5 pr-7 focus:outline-none focus:border-blue-500 cursor-pointer shadow-sm hover:border-gray-500 disabled:opacity-60 disabled:cursor-not-allowed transition shrink-0"
            style={{
              backgroundImage: `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%239ca3af' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E")`,
              backgroundRepeat: 'no-repeat',
              backgroundPosition: 'right 0.5rem center',
            }}
            title="Agent Harness"
          >
            {agents.map((a) => (
              <option key={a.id} value={a.id}>{a.name}</option>
            ))}
          </select>

          {/* Model selector — pick model for selected agent */}
          <select
            value={effectiveModelId}
            onChange={(e) => handleModelChange(e.target.value)}
            disabled={sending || !currentAgent}
            className="appearance-none bg-background border border-blue-500/50 text-blue-400 text-xs font-medium rounded-md py-1.5 pl-2.5 pr-7 focus:outline-none focus:border-blue-400 cursor-pointer shadow-sm hover:border-blue-400 disabled:opacity-60 disabled:cursor-not-allowed transition shrink-0"
            style={{
              backgroundImage: `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%2360a5fa' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E")`,
              backgroundRepeat: 'no-repeat',
              backgroundPosition: 'right 0.5rem center',
            }}
            title="Model"
          >
            {currentAgent?.models.map((m) => (
              <option key={m.id} value={m.id}>{m.name}</option>
            ))}
          </select>

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
            className="p-1.5 text-gray-400 hover:text-white bg-gray-800 rounded-md transition relative"
            title="Chat History"
          >
            <Menu className="w-4 h-4" />
          </button>
        </div>

        {/* Chat list popout (floats over chat messages) */}
        <ChatHistory
          sessions={sessions}
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
      <div className="flex-1 overflow-y-auto p-3 lg:p-4 space-y-3 lg:space-y-4 pb-20 lg:pb-4">
        {mergedEvents.length === 0 && (
          <div className="rounded-lg border border-gray-800 bg-panel/50 p-3 text-xs text-gray-500">
            Send a message to start a conversation.
          </div>
        )}
        {mergedEvents.map((event, i) => (
          <ChatMessageItem
            key={i}
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

      {/* Chat Input (Blueprint Sec 17 — input composer) */}
      <div className="p-2.5 lg:p-3 bg-gradient-to-t from-background to-transparent shrink-0 border-t border-gray-800/50 pb-20 lg:pb-3">
        <div className="relative flex items-end gap-2">
          <button
            className="p-2.5 bg-panel border border-gray-700 rounded-xl hover:bg-gray-800 hover:border-gray-500 transition text-gray-400 shrink-0"
            title="Upload Artifact"
          >
            <Paperclip className="w-4 h-4" />
          </button>
          <div className="relative flex-1">
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={agents.length === 0 ? 'Configure an agent first...' : 'Message agent...'}
              disabled={sending || agents.length === 0}
              className="w-full bg-panel border border-gray-700 rounded-xl pl-3 pr-10 py-3 text-sm focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 resize-none h-12 disabled:opacity-60 disabled:cursor-not-allowed transition-all"
            />
            {agentRunning && activeSessionId ? (
              <button
                onClick={handleStop}
                className="absolute right-2 bottom-2 p-1.5 bg-red-600 rounded-lg hover:bg-red-500 transition"
                title="Stop"
              >
                <Square className="w-3.5 h-3.5 text-white" />
              </button>
            ) : (
              <button
                onClick={handleSend}
                disabled={!canSend}
                className="absolute right-2 bottom-2 p-1.5 bg-blue-600 rounded-lg hover:bg-blue-500 disabled:bg-gray-700 disabled:cursor-not-allowed transition"
              >
                <ArrowUp className="w-3.5 h-3.5 text-white" />
              </button>
            )}
          </div>
        </div>
      </div>
    </aside>
  )
}
