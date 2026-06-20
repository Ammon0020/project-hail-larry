import { useState, type KeyboardEvent } from 'react'
import { Menu, Paperclip, ArrowUp } from 'lucide-react'
import { cn } from '@/lib/utils'
import { ChatMessageItem } from './ChatMessageItem'
import { ChatHistory } from './ChatHistory'
import type { AppEvent, Agent, Session } from '@/types'

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
  onSendMessage,
  onPermissionResponse,
}: {
  events: AppEvent[]
  agents: Agent[]
  sessions: Session[]
  visible: boolean
  onSendMessage: (content: string) => void
  onPermissionResponse: (sessionId: string, decision: 'allow' | 'deny') => void
}) {
  const [selectedAgent, setSelectedAgent] = useState(agents[0]?.id ?? '')
  const [selectedModel, setSelectedModel] = useState(agents[0]?.models[0]?.id ?? '')
  const [chatHistoryOpen, setChatHistoryOpen] = useState(false)
  const [input, setInput] = useState('')

  /** Updates available models when the agent harness changes. */
  const handleAgentChange = (agentId: string) => {
    setSelectedAgent(agentId)
    const agent = agents.find((a) => a.id === agentId)
    if (agent) setSelectedModel(agent.models[0]?.id ?? '')
  }

  const handleSend = () => {
    const content = input.trim()
    if (!content) return
    onSendMessage(content)
    setInput('')
  }

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  const currentAgent = agents.find((a) => a.id === selectedAgent)

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
            value={selectedAgent}
            onChange={(e) => handleAgentChange(e.target.value)}
            className="appearance-none bg-background border border-gray-700 text-gray-200 text-xs font-semibold rounded-md py-1.5 pl-2.5 pr-7 focus:outline-none focus:border-blue-500 cursor-pointer shadow-sm hover:border-gray-500 transition shrink-0"
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
            value={selectedModel}
            onChange={(e) => setSelectedModel(e.target.value)}
            className="appearance-none bg-background border border-blue-500/50 text-blue-400 text-xs font-medium rounded-md py-1.5 pl-2.5 pr-7 focus:outline-none focus:border-blue-400 cursor-pointer shadow-sm hover:border-blue-400 transition shrink-0"
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

          {/* Hamburger menu — toggles chat list popout */}
          <button
            onClick={() => setChatHistoryOpen(!chatHistoryOpen)}
            className="p-1.5 text-gray-400 hover:text-white bg-gray-800 rounded-md transition relative"
            title="Chat History"
          >
            <Menu className="w-4 h-4" />
            <div className="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full bg-blue-500 animate-pulse" />
          </button>
        </div>

        {/* Chat list popout (floats over chat messages) */}
        <ChatHistory
          sessions={sessions}
          open={chatHistoryOpen}
          onClose={() => setChatHistoryOpen(false)}
        />
      </div>

      {/* Chat Messages — rendered from event stream (Blueprint Sec 11) */}
      <div className="flex-1 overflow-y-auto p-3 lg:p-4 space-y-3 lg:space-y-4 pb-20 lg:pb-4">
        {events.map((event, i) => (
          <ChatMessageItem
            key={i}
            event={event}
            onPermissionResponse={onPermissionResponse}
          />
        ))}
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
              placeholder="Message agent..."
              className="w-full bg-panel border border-gray-700 rounded-xl pl-3 pr-10 py-3 text-sm focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 resize-none h-12 transition-all"
            />
            <button
              onClick={handleSend}
              className="absolute right-2 bottom-2 p-1.5 bg-blue-600 rounded-lg hover:bg-blue-500 transition"
            >
              <ArrowUp className="w-3.5 h-3.5 text-white" />
            </button>
          </div>
        </div>
      </div>
    </aside>
  )
}
