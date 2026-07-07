import { useState, useEffect, type KeyboardEvent as ReactKeyboardEvent } from 'react'
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
  workspaces,
  open,
  onClose,
  onCreateSession,
  onSelectSession,
  onRenameSession,
  onDeleteSession,
  onExportSession,
}: {
  sessions: Session[]
  workspaces: { id: string; name: string }[]
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
  // Workspace filter for the session list. "all" shows sessions from every
  // workspace (the show-all model — sessions keep running when switching
  // workspaces). Reset to "all" each time the popout opens so the choice does
  // not silently persist across opens. Uses the "adjust state during render"
  // pattern (React docs) instead of setState-in-effect to avoid cascading
  // renders and the ESLint rule react-hooks/set-state-in-effect.
  const [workspaceFilter, setWorkspaceFilter] = useState('all')
  const [prevOpen, setPrevOpen] = useState(open)
  if (open !== prevOpen) {
    setPrevOpen(open)
    if (open) setWorkspaceFilter('all')
  }

  // Look up a workspace display name by id; falls back to the raw id when the
  // workspace is unknown (e.g. a session whose workspace was unregistered).
  const workspaceName = (id: string): string =>
    workspaces.find((w) => w.id === id)?.name ?? id

  // Filter the session list by the selected workspace. Sessions with no
  // workspace field (legacy) are only shown under "all" — they don't match a
  // specific workspace filter.
  const filteredSessions =
    workspaceFilter === 'all'
      ? sessions
      : sessions.filter((s) => s.workspace === workspaceFilter)

  // Close the popout on Escape (keyboard accessibility — AGENTS.md a11y focus).
  useEffect(() => {
    if (!open) return
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        onClose()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [open, onClose])

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

  const handleRenameKey = (e: ReactKeyboardEvent<HTMLInputElement>) => {
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
        'absolute top-full left-0 right-0 z-50 bg-panel border-b border-border shadow-lg max-h-[60vh] overflow-y-auto',
        open ? 'block' : 'hidden',
      )}
      role="dialog"
      aria-label="Chat history"
    >
      <div className="p-2 space-y-1">
        <div className="flex items-center justify-between px-1 pb-1">
          <div className="text-[10px] text-muted-foreground uppercase font-bold tracking-wider">Chat History</div>
          <button onClick={onClose} className="text-muted-foreground hover:text-foreground transition" aria-label="Close chat history">
            <X className="w-3.5 h-3.5" />
          </button>
        </div>

        {/* Workspace filter — show-all model: sessions from every workspace
            are listed, with an optional filter to narrow to one. */}
        <select
          value={workspaceFilter}
          onChange={(e) => setWorkspaceFilter(e.target.value)}
          className="select-chevron appearance-none w-full bg-background border border-input text-muted-foreground text-xs rounded-md py-1.5 pl-2.5 pr-7 focus:outline-none focus:border-ring cursor-pointer mb-1"
          aria-label="Filter sessions by workspace"
        >
          <option value="all">All Workspaces</option>
          {workspaces.map((w) => (
            <option key={w.id} value={w.id}>{w.name}</option>
          ))}
        </select>

        {/* Session rows */}
        {filteredSessions.map((s) => (
          <div
            key={s.id}
            className={cn(
              'flex items-center justify-between p-2 rounded-lg session-item group',
              s.active
                ? 'bg-primary/10 border border-primary/20'
                : 'hover:bg-accent',
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
                  className="flex-1 bg-background text-xs text-foreground rounded px-1 py-0.5 focus:outline-none focus:ring-1 focus:ring-ring"
                />
                <button onMouseDown={(e) => e.preventDefault()} onClick={commitRename} className="text-muted-foreground hover:text-green-400" title="Save" aria-label="Save rename">
                  <Check className="w-3.5 h-3.5" />
                </button>
              </div>
            ) : confirmId === s.id ? (
              // Delete confirmation.
              <div className="flex items-center gap-2 flex-1 text-xs">
                <span className="text-destructive truncate flex-1">Delete "{s.name}"?</span>
                <button
                  onClick={() => { setConfirmId(null); onDeleteSession(s.id) }}
                  className="text-destructive hover:text-destructive/80 font-medium"
                >
                  Delete
                </button>
                <button onClick={() => setConfirmId(null)} className="text-muted-foreground hover:text-foreground">
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
                    s.active ? 'font-medium text-primary' : 'text-muted-foreground',
                  )}>
                    {s.name}
                  </span>
                  {s.workspace && (
                    <span className="shrink-0 text-muted-foreground bg-muted px-1.5 py-0.5 rounded text-[10px]">
                      {workspaceName(s.workspace)}
                    </span>
                  )}
                </button>
                <div className="flex items-center gap-1 shrink-0 opacity-0 group-hover:opacity-100 transition">
                  {s.modelId && <span className="text-[10px] text-muted-foreground mr-1">{s.modelId}</span>}
                  <button onClick={() => startRename(s)} className="text-muted-foreground hover:text-foreground" title="Rename" aria-label={`Rename ${s.name}`}>
                    <Pencil className="w-3 h-3" />
                  </button>
                  <button onClick={() => onExportSession(s.id)} className="text-muted-foreground hover:text-foreground" title="Export" aria-label={`Export ${s.name}`}>
                    <Download className="w-3 h-3" />
                  </button>
                  <button onClick={() => setConfirmId(s.id)} className="text-muted-foreground hover:text-destructive" title="Delete" aria-label={`Delete ${s.name}`}>
                    <Trash2 className="w-3 h-3" />
                  </button>
                </div>
              </>
            )}
          </div>
        ))}

        {/* New chat button */}
        <button
          className="w-full flex items-center gap-2 p-2 rounded-lg hover:bg-accent cursor-pointer text-muted-foreground text-xs transition"
          onClick={onCreateSession}
        >
          <Plus className="w-3.5 h-3.5" /> New Chat
        </button>
      </div>
    </div>
  )
}
