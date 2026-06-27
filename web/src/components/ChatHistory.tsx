import { useState, type KeyboardEvent } from 'react'
import { Plus, X, Pencil, Trash2, Check, Download } from 'lucide-react'
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
  onRenameSession,
  onDeleteSession,
  onExportSession,
}: {
  sessions: Session[]
  open: boolean
  onClose: () => void
  onCreateSession: () => void
  onSelectSession: (sessionId: string) => void
  onRenameSession: (sessionId: string, name: string) => void
  onDeleteSession: (sessionId: string) => void
  onExportSession: (sessionId: string) => void
}) {
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editValue, setEditValue] = useState('')
  const [confirmId, setConfirmId] = useState<string | null>(null)

  const startRename = (s: Session) => {
    setEditingId(s.id)
    setEditValue(s.name)
  }

  const commitRename = () => {
    if (editingId && editValue.trim()) {
      onRenameSession(editingId, editValue.trim())
    }
    setEditingId(null)
  }

  const handleRenameKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      commitRename()
    } else if (e.key === 'Escape') {
      setEditingId(null)
    }
  }

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
              'flex items-center justify-between p-2 rounded-lg session-item group',
              s.active
                ? 'bg-blue-600/10 border border-blue-500/20'
                : 'hover:bg-gray-800/50',
            )}
          >
            {editingId === s.id ? (
              // Inline rename input.
              <div className="flex items-center gap-1.5 flex-1">
                <div className={cn('w-1.5 h-1.5 rounded-full shrink-0', statusDotClass[s.status])} />
                <input
                  ref={(el) => el?.focus()}
                  value={editValue}
                  onChange={(e) => setEditValue(e.target.value)}
                  onKeyDown={handleRenameKey}
                  onBlur={commitRename}
                  className="flex-1 bg-black/30 text-xs text-gray-200 rounded px-1 py-0.5 focus:outline-none focus:ring-1 focus:ring-blue-500"
                />
                <button onMouseDown={(e) => e.preventDefault()} onClick={commitRename} className="text-gray-400 hover:text-green-400" title="Save">
                  <Check className="w-3.5 h-3.5" />
                </button>
              </div>
            ) : confirmId === s.id ? (
              // Delete confirmation.
              <div className="flex items-center gap-2 flex-1 text-xs">
                <span className="text-red-300 truncate flex-1">Delete "{s.name}"?</span>
                <button
                  onClick={() => { setConfirmId(null); onDeleteSession(s.id) }}
                  className="text-red-400 hover:text-red-300 font-medium"
                >
                  Delete
                </button>
                <button onClick={() => setConfirmId(null)} className="text-gray-400 hover:text-white">
                  Cancel
                </button>
              </div>
            ) : (
              <>
                <button
                  className="flex items-center gap-2 overflow-hidden flex-1 text-left cursor-pointer"
                  onClick={() => onSelectSession(s.id)}
                >
                  <div className={cn('w-1.5 h-1.5 rounded-full shrink-0', statusDotClass[s.status])} />
                  <span className={cn(
                    'truncate text-xs',
                    s.active ? 'font-medium text-blue-400' : 'text-gray-400',
                  )}>
                    {s.name}
                  </span>
                </button>
                <div className="flex items-center gap-1 shrink-0 opacity-0 group-hover:opacity-100 transition">
                  {s.modelId && <span className="text-[10px] text-gray-600 mr-1">{s.modelId}</span>}
                  <button onClick={() => startRename(s)} className="text-gray-500 hover:text-white" title="Rename">
                    <Pencil className="w-3 h-3" />
                  </button>
                  <button onClick={() => onExportSession(s.id)} className="text-gray-500 hover:text-white" title="Export">
                    <Download className="w-3 h-3" />
                  </button>
                  <button onClick={() => setConfirmId(s.id)} className="text-gray-500 hover:text-red-400" title="Delete">
                    <Trash2 className="w-3 h-3" />
                  </button>
                </div>
              </>
            )}
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
