import { useState, useEffect, useRef, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { Plus, X, Pencil, Trash2, Check, Download, Search, ChevronDown, ChevronUp } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { SessionHistoryCapabilities } from '@/lib/api'
import type { Session, SessionStatus } from '@/types'

/** Maps session status to status dot styling. Mirrors backend
 *  `SessionState` — 6 real states, no phantoms. */
const statusDotClass: Record<SessionStatus, string> = {
  created: 'bg-gray-600',
  idle: 'bg-gray-400',
  running: 'bg-blue-400 animate-pulse',
  interrupted: 'bg-orange-400',
  failed: 'bg-red-400',
  closed: 'bg-gray-700',
}

/** Cap for the "recent" preview list. The full list (with search) is shown
 *  after the user clicks "see more". */
const RECENT_CAP = 6

interface SessionRowProps {
  session: Session
  editing: boolean
  editValue: string
  setEditValue: (v: string) => void
  onRenameKey: (e: ReactKeyboardEvent<HTMLInputElement>) => void
  onCommitRename: () => void
  confirmingDelete: boolean
  onConfirmDelete: () => void
  onCancelDelete: () => void
  onStartRename: () => void
  onExport: () => void
  onRequestDelete: () => void
  onSelect: () => void
  workspaceName: (id: string) => string
}

/**
 * Renders a single session row in the history list. Extracted so the recent
 * preview list and the full searchable list share the exact same row markup
 * (rename, delete-confirm, export, status dot, workspace chip).
 */
function SessionRow({
  session: s,
  editing,
  editValue,
  setEditValue,
  onRenameKey,
  onCommitRename,
  confirmingDelete,
  onConfirmDelete,
  onCancelDelete,
  onStartRename,
  onExport,
  onRequestDelete,
  onSelect,
  workspaceName,
}: SessionRowProps) {
  const inputRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    if (editing) inputRef.current?.focus()
  }, [editing])
  return (
    <div
      className={cn(
        'session-item group flex items-center justify-between rounded-lg p-2',
        s.active
          ? 'border border-primary/20 bg-primary/10'
          : 'hover:bg-accent',
      )}
    >
      {editing ? (
        // Inline rename input.
        <div className="flex flex-1 items-center gap-1.5">
          <div className={cn('size-1.5 shrink-0 rounded-full', statusDotClass[s.status])} />
          <input
            ref={inputRef}
            value={editValue}
            onChange={(e) => setEditValue(e.target.value)}
            onKeyDown={onRenameKey}
            onBlur={onCommitRename}
            className="flex-1 rounded bg-background px-1 py-0.5 text-xs text-foreground focus:ring-1 focus:ring-ring focus:outline-none"
          />
          <button onMouseDown={(e) => e.preventDefault()} onClick={onCommitRename} className="text-muted-foreground hover:text-green-400" title="Save" aria-label="Save rename">
            <Check className="size-3.5" />
          </button>
        </div>
      ) : confirmingDelete ? (
        // Delete confirmation.
        <div className="flex flex-1 items-center gap-2 text-xs">
          <span className="flex-1 truncate text-destructive">Delete "{s.name}"?</span>
          <button
            onClick={onConfirmDelete}
            className="font-medium text-destructive hover:text-destructive/80"
          >
            Delete
          </button>
          <button onClick={onCancelDelete} className="text-muted-foreground hover:text-foreground">
            Cancel
          </button>
        </div>
      ) : (
        <>
          <button
            className="flex flex-1 cursor-pointer items-center gap-2 overflow-hidden text-left"
            onClick={onSelect}
          >
            <div className={cn('size-1.5 shrink-0 rounded-full', statusDotClass[s.status])} />
            <span className={cn(
              'truncate text-xs',
              s.active ? 'font-medium text-primary' : 'text-muted-foreground',
            )}>
              {s.name}
            </span>
            {s.workspace && (
              <span className="shrink-0 rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                {workspaceName(s.workspace)}
              </span>
            )}
          </button>
          <div className="flex shrink-0 items-center gap-1 opacity-0 transition group-hover:opacity-100">
            {s.modelId && <span className="mr-1 text-[10px] text-muted-foreground">{s.modelId}</span>}
            <button onClick={onStartRename} className="text-muted-foreground hover:text-foreground" title="Rename" aria-label={`Rename ${s.name}`}>
              <Pencil className="size-3" />
            </button>
            <button onClick={onExport} className="text-muted-foreground hover:text-foreground" title="Export" aria-label={`Export ${s.name}`}>
              <Download className="size-3" />
            </button>
            <button onClick={onRequestDelete} className="text-muted-foreground hover:text-destructive" title="Delete" aria-label={`Delete ${s.name}`}>
              <Trash2 className="size-3" />
            </button>
          </div>
        </>
      )}
    </div>
  )
}

/**
 * Chat history popout — session list (Blueprint Sec 17 — chat history popout).
 * Triggered from the ChatTabBar history button (after the WI-3 restructure).
 *
 * Two views:
 * - **Recent** (default): the most recently touched sessions, capped at
 *   `RECENT_CAP`. A "see more" button at the bottom expands to the full list.
 * - **Full**: all sessions with a search input that filters by name. A
 *   "show less" button collapses back to the recent view.
 *
 * Rename / delete / export functionality is unchanged from before.
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
  historyCapabilities,
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
  /** Live caps for the selected session; unavailable means the agent is cold. */
  historyCapabilities?: SessionHistoryCapabilities
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
  // Expanded view — when true, shows the full searchable list instead of the
  // recent-capped preview. Reset to false each time the popout opens so the
  // user always lands on the compact recent view first.
  const [expanded, setExpanded] = useState(false)
  const [search, setSearch] = useState('')
  if (open !== prevOpen) {
    setPrevOpen(open)
    if (open) {
      setWorkspaceFilter('all')
      setExpanded(false)
      setSearch('')
    }
  }

  // Look up a workspace display name by id; falls back to the raw id when the
  // workspace is unknown (e.g. a session whose workspace was unregistered).
  const workspaceName = (id: string): string =>
    workspaces.find((w) => w.id === id)?.name ?? id
  const historyFallbackMessage = !historyCapabilities?.available
    ? null
    : !historyCapabilities.canListSessions
      ? 'This agent cannot browse its saved sessions. Showing Local Agent history.'
      : !historyCapabilities.canLoadSession
        ? 'This agent cannot reopen agent-owned sessions. Showing Local Agent history.'
        : null

  // Filter the session list by the selected workspace. Sessions with no
  // workspace field (legacy) are only shown under "all" — they don't match a
  // specific workspace filter.
  const workspaceFiltered =
    workspaceFilter === 'all'
      ? sessions
      : sessions.filter((s) => s.workspace === workspaceFilter)

  // Recent view: most recently touched sessions first, capped. The backend
  // doesn't currently expose a "lastActivityAt" — `time` is the creation
  // timestamp, and active sessions sort to the top via the `active` flag.
  // Sort: active first, then by descending creation time.
  const recentSessions = [...workspaceFiltered]
    .sort((a, b) => {
      if (a.active && !b.active) return -1
      if (!a.active && b.active) return 1
      return (b.time || '').localeCompare(a.time || '')
    })
    .slice(0, RECENT_CAP)

  // Expanded view: apply the search filter on top of the workspace filter.
  const searchTrim = search.trim().toLowerCase()
  const fullSessions =
    searchTrim.length === 0
      ? workspaceFiltered
      : workspaceFiltered.filter((s) => s.name.toLowerCase().includes(searchTrim))

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
      e.preventDefault()
      e.stopPropagation()
      setEditingId(null)
    }
  }

  /** Renders a SessionRow wired to this popout's rename/delete/export state. */
  const renderRow = (s: Session) => (
    <SessionRow
      key={s.id}
      session={s}
      editing={editingId === s.id}
      editValue={editValue}
      setEditValue={setEditValue}
      onRenameKey={handleRenameKey}
      onCommitRename={commitRename}
      confirmingDelete={confirmId === s.id}
      onConfirmDelete={() => { setConfirmId(null); onDeleteSession(s.id) }}
      onCancelDelete={() => setConfirmId(null)}
      onStartRename={() => startRename(s)}
      onExport={() => onExportSession(s.id)}
      onRequestDelete={() => setConfirmId(s.id)}
      onSelect={() => onSelectSession(s.id)}
      workspaceName={workspaceName}
    />
  )

  return (
    <div
      className={cn(
        'absolute inset-x-0 top-full z-50 max-h-[60vh] overflow-y-auto border-b border-border bg-panel shadow-lg',
        open ? 'block' : 'hidden',
      )}
      role="dialog"
      aria-label="Chat history"
    >
      <div className="space-y-1 p-2">
        <div className="flex items-center justify-between px-1 pb-1">
          <div className="text-[10px] font-bold tracking-wider text-muted-foreground uppercase">Chat History</div>
          <button onClick={onClose} className="text-muted-foreground transition hover:text-foreground" aria-label="Close chat history">
            <X className="size-3.5" />
          </button>
        </div>

        {historyFallbackMessage && (
          <p className="rounded bg-muted px-2 py-1.5 text-xs text-muted-foreground" role="status">
            {historyFallbackMessage}
          </p>
        )}

        {/* Workspace filter — show-all model: sessions from every workspace
            are listed, with an optional filter to narrow to one. */}
        <select
          value={workspaceFilter}
          onChange={(e) => setWorkspaceFilter(e.target.value)}
          className="select-chevron mb-1 w-full cursor-pointer appearance-none rounded-md border border-input bg-background py-1.5 pr-7 pl-2.5 text-xs text-muted-foreground focus:border-ring focus:outline-none"
          aria-label="Filter sessions by workspace"
        >
          <option value="all">All Workspaces</option>
          {workspaces.map((w) => (
            <option key={w.id} value={w.id}>{w.name}</option>
          ))}
        </select>

        {/* Search input — only shown in the expanded (full) view. */}
        {expanded && (
          <div className="relative mb-1">
            <Search className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search conversations..."
              className="w-full rounded-md border border-input bg-background py-1.5 pr-2.5 pl-7 text-xs text-foreground focus:border-ring focus:outline-none"
              aria-label="Search sessions"
            />
          </div>
        )}

        {/* Session rows — recent (capped) or full (search-filtered). */}
        {(expanded ? fullSessions : recentSessions).map(renderRow)}

        {/* Empty state for the full search view. */}
        {expanded && fullSessions.length === 0 && (
          <div className="px-2 py-3 text-center text-xs text-muted-foreground">
            No conversations match "{search}".
          </div>
        )}

        {/* See more / show less toggle. Only shown when there are more
            sessions than the recent cap (otherwise the toggle is a no-op). */}
        {workspaceFiltered.length > RECENT_CAP && (
          <button
            onClick={() => setExpanded((v) => !v)}
            className="flex w-full cursor-pointer items-center justify-center gap-1 rounded-lg p-2 text-xs text-muted-foreground transition hover:bg-accent hover:text-foreground"
          >
            {expanded ? (
              <>
                <ChevronUp className="size-3.5" /> Show less
              </>
            ) : (
              <>
                <ChevronDown className="size-3.5" /> See more ({
                  workspaceFiltered.length - RECENT_CAP
                })
              </>
            )}
          </button>
        )}

        {/* New chat button */}
        <button
          className="flex w-full cursor-pointer items-center gap-2 rounded-lg p-2 text-xs text-muted-foreground transition hover:bg-accent"
          onClick={onCreateSession}
        >
          <Plus className="size-3.5" /> New Chat
        </button>
      </div>
    </div>
  )
}
