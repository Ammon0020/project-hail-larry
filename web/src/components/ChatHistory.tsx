import { Plus, X } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { Session, SessionStatus } from '@/types'

/** Maps session status to status dot styling. */
const statusDotClass: Record<SessionStatus, string> = {
  created: 'bg-gray-600',
  starting: 'bg-yellow-400 animate-pulse',
  running: 'bg-blue-400 animate-pulse',
  waiting_permission: 'bg-yellow-400 animate-pulse',
  interrupted: 'bg-orange-400',
  completed: 'bg-gray-600',
  failed: 'bg-red-400',
  archived: 'bg-gray-600',
}

/**
 * Chat history popout — session list (Blueprint Sec 17 — chat history popout).
 * Floats over the chat messages when toggled.
 * Shows past and active sessions with status indicators.
 */
export function ChatHistory({
  sessions,
  open,
  onClose,
  onCreateSession,
  onSelectSession,
}: {
  sessions: Session[]
  open: boolean
  onClose: () => void
  onCreateSession: () => void
  onSelectSession: (sessionId: string) => void
}) {
  return (
    <div
      className={cn(
        'absolute top-full left-0 right-0 z-50 bg-panel border-b border-gray-700 shadow-lg max-h-[60vh] overflow-y-auto',
        open ? 'block' : 'hidden',
      )}
    >
      {/* Resize handle */}
      <div className="h-1 bg-gray-700 cursor-ns-resize shrink-0 hover:bg-blue-500" />

      <div className="p-2 space-y-1">
        <div className="flex items-center justify-between px-1 pb-1">
          <div className="text-[10px] text-gray-500 uppercase font-bold tracking-wider">Chat History</div>
          <button onClick={onClose} className="text-gray-500 hover:text-white transition">
            <X className="w-3.5 h-3.5" />
          </button>
        </div>

        {/* Session rows */}
        {sessions.map((s) => (
          <div
            key={s.id}
            className={cn(
              'flex items-center justify-between p-2 rounded-lg cursor-pointer session-item group',
              s.active
                ? 'bg-blue-600/10 border border-blue-500/20'
                : 'hover:bg-gray-800/50',
            )}
            onClick={() => onSelectSession(s.id)}
          >
            <div className="flex items-center gap-2 overflow-hidden">
              <div className={cn('w-1.5 h-1.5 rounded-full shrink-0', statusDotClass[s.status])} />
              <span className={cn(
                'truncate text-xs',
                s.active ? 'font-medium text-blue-400' : 'text-gray-400',
              )}>
                {s.name}
              </span>
            </div>
            <span className="text-[10px] text-gray-500 shrink-0">{s.time}</span>
          </div>
        ))}

        {/* New chat button */}
        <button
          className="w-full flex items-center gap-2 p-2 rounded-lg hover:bg-gray-800/50 cursor-pointer text-gray-400 text-xs transition"
          onClick={onCreateSession}
        >
          <Plus className="w-3.5 h-3.5" /> New Chat
        </button>
      </div>
    </div>
  )
}
