import { useState, useEffect, useCallback } from 'react'
import { X } from 'lucide-react'
import { LockScreen } from '@/components/LockScreen'
import { ActivityBar } from '@/components/ActivityBar'
import { LeftSidebar } from '@/components/LeftSidebar'
import { EditorPane } from '@/components/EditorPane'
import { ChatPanel } from '@/components/ChatPanel'
import { MobileNav } from '@/components/MobileNav'
import { MobileSettings } from '@/components/MobileSettings'
import { SettingsModal } from '@/components/SettingsModal'
import { useBackend } from '@/hooks/useBackend'
import type { LeftPanel, MobileView, FileTreeNode, AppEvent, SessionStatus, Tab } from '@/types'

/**
 * Runtime guard that narrows an arbitrary backend status string to the
 * SessionStatus union. Falls back to 'created' for unknown values so a
 * malformed status never yields an undefined className lookup in
 * ChatHistory's statusDotClass map (AGENTS.md — type safety).
 */
const VALID_SESSION_STATUSES: readonly SessionStatus[] = [
  'created', 'starting', 'running', 'waiting_permission',
  'interrupted', 'completed', 'failed', 'archived',
]
function narrowStatus(raw: string): SessionStatus {
  return (VALID_SESSION_STATUSES as readonly string[]).includes(raw)
    ? (raw as SessionStatus)
    : 'created'
}

/**
 * Reads a localStorage key and validates it against a list of allowed values.
 * Returns the validated value or the fallback. Avoids unsafe `as` casts on
 * arbitrary stored strings (AGENTS.md — type safety).
 */
function readValidString<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const v = localStorage.getItem(key)
  return v && (allowed as readonly string[]).includes(v) ? (v as T) : fallback
}

/**
 * App shell — the Local Agent Interface (Blueprint Sec 17).
 *
 * Two top-level views:
 * 1. Lock screen — shown to unpaired devices (Blueprint Sec 19)
 * 2. Main app — VS Code-style layout for paired devices
 *
 * Desktop: activity bar + left sidebar + editor + right chat panel
 * Mobile: bottom-nav layout, one panel at a time
 */
export default function App() {
  // Pairing state — check localStorage for existing credential (Blueprint Sec 19).
  const [paired, setPaired] = useState(() => {
    return !!localStorage.getItem('lai:deviceCredential')
  })

  // Panel state — restored from localStorage so the layout survives a reload.
  // Validated against the allowed union values so a corrupted/old entry cannot
  // produce an invalid panel id (AGENTS.md — type safety).
  const [leftPanel, setLeftPanel] = useState<LeftPanel>(
    () => readValidString('lai:leftPanel', ['files', 'search'] as const, 'files'),
  )
  const [mobileView, setMobileView] = useState<MobileView>(
    () => readValidString('lai:mobileView', ['explorer', 'editor', 'chat', 'settings'] as const, 'editor'),
  )
  const [isDesktop, setIsDesktop] = useState(
    typeof window !== 'undefined' ? window.innerWidth >= 1024 : true,
  )
  const [isSettingsModalOpen, setIsSettingsModalOpen] = useState(false)

  // Resizable panel widths — restored from localStorage so the layout
  // survives a reload (Feature 2). Defaults: 260px left, 420px right.
  const LEFT_MIN = 180
  const LEFT_MAX = 480
  const RIGHT_MIN = 300
  const RIGHT_MAX = 700
  const [leftPanelWidth, setLeftPanelWidth] = useState(() => {
    const stored = Number(localStorage.getItem('lai:leftPanelWidth'))
    return Number.isFinite(stored) && stored >= LEFT_MIN && stored <= LEFT_MAX
      ? stored
      : 260
  })
  const [rightPanelWidth, setRightPanelWidth] = useState(() => {
    const stored = Number(localStorage.getItem('lai:rightPanelWidth'))
    return Number.isFinite(stored) && stored >= RIGHT_MIN && stored <= RIGHT_MAX
      ? stored
      : 420
  })

  // Tab state — restored from localStorage so open files survive a reload.
  const [openTabs, setOpenTabs] = useState<Tab[]>(() => {
    try {
      const stored = localStorage.getItem('lai:openTabs')
      return stored ? (JSON.parse(stored) as Tab[]) : []
    } catch {
      return []
    }
  })
  const [activeTabId, setActiveTabId] = useState<string | null>(
    () => localStorage.getItem('lai:activeTabId') || null,
  )

  // Save error — shown as a transient banner so save failures aren't silent
  // (previously only console.error'd, making debugging impossible).
  const [saveError, setSaveError] = useState<string | null>(null)

  // Line to scroll to when a search result is clicked. Set by
  // handleSearchResultSelect and consumed by EditorPane's scrollToLine prop.
  // Cleared after the editor processes the jump so the same line number can
  // re-trigger (e.g. clicking a result in a different file at the same line).
  const [searchResultLine, setSearchResultLine] = useState<number | null>(null)

  // Session state — restored from localStorage so the active conversation
  // survives a page reload (UI Spec §6.2).
  const [activeSessionId, setActiveSessionId] = useState<string | null>(
    () => localStorage.getItem('lai:activeSessionId') || null,
  )

  // Real backend connection
  const backend = useBackend()

  /** Track viewport changes for responsive layout switching. */
  useEffect(() => {
    const handleResize = () => setIsDesktop(window.innerWidth >= 1024)
    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [])

  /** Persist the active conversation so it is restored on reload. */
  useEffect(() => {
    if (activeSessionId) localStorage.setItem('lai:activeSessionId', activeSessionId)
    else localStorage.removeItem('lai:activeSessionId')
  }, [activeSessionId])

  // Validate the persisted activeSessionId against the backend's session list.
  // After a daemon restart the session may no longer exist (e.g.
  // conversations.json was wiped or the session was deleted). If the loaded
  // list is non-empty and does not contain the active id, clear it so the UI
  // shows the new-chat state instead of sending prompts to a dead session.
  // Uses the "adjust state during render" pattern from the React docs instead
  // of setState-in-effect to avoid cascading renders. The empty-list case is
  // handled defensively by sendPrompt's 404 path in useBackend.
  const [prevSessions, setPrevSessions] = useState(backend.sessions)
  // Tracks the session id whose events we've already loaded so we only trigger
  // loadSessionEvents once per active session (not on every render). Reset to
  // null when activeSessionId changes so a freshly-selected session reloads.
  const [loadedEventsForSession, setLoadedEventsForSession] = useState<string | null>(null)
  if (activeSessionId !== loadedEventsForSession && activeSessionId === null) {
    // Switched to "new chat" — no session to load events for.
    setLoadedEventsForSession(null)
  }
  if (backend.sessions !== prevSessions) {
    setPrevSessions(backend.sessions)
    if (
      activeSessionId &&
      backend.sessions.length > 0 &&
      !backend.sessions.some((s) => s.id === activeSessionId)
    ) {
      setActiveSessionId(null)
      setLoadedEventsForSession(null)
    } else if (
      // Only load session events on the INITIAL sessions load — i.e. when the
      // session list transitions from empty to populated (page reload). A newly
      // created session is added to an already-populated list and has no
      // persisted history to fetch; its events arrive in real time via
      // WebSocket. Calling loadSessionEvents for a brand-new session races with
      // the in-flight prompt POST: the fetch returns an empty list (the
      // PromptSubmitted event hasn't been persisted yet) and overwrites the
      // WebSocket-delivered event, making the user's message flash then vanish.
      prevSessions.length === 0 &&
      activeSessionId &&
      backend.sessions.some((s) => s.id === activeSessionId) &&
      loadedEventsForSession !== activeSessionId
    ) {
      // The session list just loaded for the first time and contains the active
      // session, but we haven't fetched its events yet. This is the reload
      // path: the persisted activeSessionId is restored from localStorage, but
      // the global loadEvents() only fetches the first 200 events across ALL
      // sessions, so the active conversation's history may be missing. Fetch it
      // explicitly. Uses the "adjust state during render" pattern (React docs)
      // instead of setState-in-effect to avoid cascading renders and the ESLint
      // rule react-hooks/set-state-in-effect.
      setLoadedEventsForSession(activeSessionId)
      backend.loadSessionEvents(activeSessionId)
    }
  }

  /** Persist open tabs, active tab, panel, and mobile view so the layout
   *  survives a page reload (UI Spec §6.2 — UI Persistence). */
  useEffect(() => {
    try {
      localStorage.setItem('lai:openTabs', JSON.stringify(openTabs))
    } catch {
      // Ignore serialization errors (e.g. quota exceeded).
    }
  }, [openTabs])

  useEffect(() => {
    if (activeTabId) localStorage.setItem('lai:activeTabId', activeTabId)
    else localStorage.removeItem('lai:activeTabId')
  }, [activeTabId])

  // Report open files and recent (unsaved) edits to the backend so the context
  // middleware can inject them into the next agent prompt. Debounced inside
  // backend.reportContext (~1s) so rapid tab switches don't flood the API.
  // Skipped when there's no active session or no active workspace.
  useEffect(() => {
    if (!activeSessionId || !backend.activeWorkspace) return
    const openFiles = openTabs.map((t) => t.path)
    const recentEdits = openTabs.filter((t) => t.unsaved).map((t) => t.path)
    backend.reportContext(activeSessionId, openFiles, recentEdits)
  }, [openTabs, activeSessionId, backend])

  useEffect(() => {
    localStorage.setItem('lai:leftPanel', leftPanel)
  }, [leftPanel])

  useEffect(() => {
    localStorage.setItem('lai:mobileView', mobileView)
  }, [mobileView])

  // Persist panel widths so the resized layout survives a reload (Feature 2).
  useEffect(() => {
    localStorage.setItem('lai:leftPanelWidth', String(leftPanelWidth))
  }, [leftPanelWidth])

  useEffect(() => {
    localStorage.setItem('lai:rightPanelWidth', String(rightPanelWidth))
  }, [rightPanelWidth])

  // Clear the search-result line target after the editor has had a chance to
  // dispatch the jump. Using setTimeout(0) defers the clear to the next
  // macrotask, which runs after the EditorPane effect that performs the
  // scroll. This ensures a subsequent click on the same line number (e.g. in
  // a different file) re-triggers the scrollToLine effect.
  useEffect(() => {
    if (searchResultLine == null) return
    const timer = setTimeout(() => setSearchResultLine(null), 0)
    return () => clearTimeout(timer)
  }, [searchResultLine])

  // ---- Tab operations ----
  // Defined before the unpaired early return so the keyboard-shortcut
  // useEffect below them is not called conditionally (react-hooks/rules-of-hooks).
  const handleTabSelect = (id: string) => setActiveTabId(id)

  const handleTabClose = useCallback(
    (id: string) => {
      setOpenTabs((prev) => {
        const next = prev.filter((t) => t.id !== id)
        if (activeTabId === id) {
          setActiveTabId(next.length > 0 ? next[next.length - 1].id : null)
        }
        return next
      })
    },
    [activeTabId],
  )

  const handleContentChange = (content: string) => {
    setOpenTabs((prev) =>
      prev.map((t) => (t.id === activeTabId ? { ...t, content, unsaved: true } : t)),
    )
  }

  const handleSave = useCallback(async () => {
    const tab = openTabs.find((t) => t.id === activeTabId)
    if (!tab) return
    try {
      const result = await backend.saveFile(tab.path, tab.content, tab.revision)
      setOpenTabs((prev) =>
        prev.map((t) =>
          t.id === activeTabId ? { ...t, revision: result.revision, unsaved: false } : t,
        ),
      )
      setSaveError(null)
    } catch (err) {
      console.error('Save failed:', err)
      setSaveError(err instanceof Error ? err.message : String(err))
    }
  }, [backend, openTabs, activeTabId])

  // ---- Global keyboard shortcuts ----
  // Registered on window so they work even when the CodeMirror editor isn't
  // focused. Ctrl+S is also handled inside CodeMirror (Prec.highest) for when
  // the editor IS focused — this is the fallback for when it isn't.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey

      // Ctrl+W — close active editor tab (prevent browser close).
      if (mod && !e.shiftKey && e.key === 'w') {
        e.preventDefault()
        if (activeTabId) handleTabClose(activeTabId)
        return
      }

      // Ctrl+S — save active file.
      if (mod && !e.shiftKey && e.key === 's') {
        e.preventDefault()
        handleSave()
        return
      }

      // Ctrl+Shift+F — switch to search panel.
      if (mod && e.shiftKey && (e.key === 'f' || e.key === 'F')) {
        e.preventDefault()
        setLeftPanel('search')
        return
      }

      // Ctrl+Shift+E — switch to explorer/files panel.
      if (mod && e.shiftKey && (e.key === 'e' || e.key === 'E')) {
        e.preventDefault()
        setLeftPanel('files')
        return
      }

      // Ctrl+B — toggle left sidebar visibility on desktop.
      if (mod && !e.shiftKey && e.key === 'b') {
        e.preventDefault()
        setLeftPanelWidth((prev) => (prev > 0 ? 0 : 260))
        return
      }
    }

    window.addEventListener('keydown', onKeyDown, true)
    return () => window.removeEventListener('keydown', onKeyDown, true)
    // handleSave and handleTabClose close over current openTabs/activeTabId,
    // so they are intentionally refreshed on every tab/active change.
  }, [activeTabId, openTabs, handleSave, handleTabClose])

  // ---- Lock screen for unpaired devices ----
  if (!paired) {
    return <LockScreen onPaired={() => setPaired(true)} />
  }

  // Convert backend file tree to the component's expected format, preserving
  // path. Validates `type` at runtime so a malformed backend node cannot
  // silently become a typed union (AGENTS.md — type safety).
  const convertNode = (n: { name: string; type: string; path?: string; children?: { name: string; type: string; path?: string; children?: unknown[] }[] }): FileTreeNode => ({
    name: n.name,
    type: n.type === 'folder' ? 'folder' : 'file',
    path: n.path,
    children: n.children?.map((c) => convertNode(c as typeof n)),
  })
  const fileTree: FileTreeNode[] = backend.fileTree.map((n) => convertNode(n))

  // ---- File operations ----
  const handleFileSelect = async (path: string) => {
    // Check if tab already open
    const existing = openTabs.find((t) => t.path === path)
    if (existing) {
      setActiveTabId(existing.id)
      return
    }
    // Load file from backend
    try {
      const file = await backend.readFile(path)
      const name = path.split(/[\\/]/).pop() || path
      const ext = name.split('.').pop() || ''
      const lang = ['js', 'jsx', 'ts', 'tsx'].includes(ext) ? 'javascript' : ext
      const tab: Tab = {
        id: path,
        name,
        path,
        content: file.content,
        revision: file.revision,
        unsaved: false,
        language: lang,
      }
      setOpenTabs((prev) => [...prev, tab])
      setActiveTabId(path)
    } catch (err) {
      console.error('Failed to open file:', err)
    }
  }

  // Opens a file from a search result and jumps the editor cursor to the
  // matched line. If the file is already open in a tab, just activates it and
  // sets the line; otherwise loads the file first, then sets the line after
  // the content is available so the editor can resolve the line position.
  const handleSearchResultSelect = async (path: string, lineNumber: number): Promise<void> => {
    const existing = openTabs.find((t) => t.path === path)
    if (existing) {
      setActiveTabId(existing.id)
    } else {
      await handleFileSelect(path)
    }
    setSearchResultLine(lineNumber)
  }

  // ---- Session operations ----
  const handleCreateSession = async (agentId: string, modelId: string): Promise<string> => {
    const session = await backend.createSession(agentId, modelId)
    setActiveSessionId(session.id)
    return session.id
  }

  const handleSelectSession = (sessionId: string) => {
    if (sessionId === '') {
      // "New Chat" — reset to a fresh state, no backend session yet
      setActiveSessionId(null)
      return
    }
    // Switching conversations filters the master event list by sessionId.
    // ws.onmessage appends ALL events for ALL sessions to eventsRef, so events
    // that arrived via WebSocket while viewing another conversation are already
    // present. However, after a daemon restart older conversations' events
    // exist only in SQLite — they were never delivered via WebSocket. The
    // initial loadEvents() fetches only the first 1000 events globally, which
    // may not include the selected conversation's history. So we explicitly
    // fetch the session's events from SQLite.
    //
    // loadSessionEvents merges by ID: it keeps events for this session whose
    // IDs are higher than the fetched max (they arrived via WebSocket after
    // the fetch started), so actively streaming sessions are not disrupted.
    setActiveSessionId(sessionId)
    backend.loadSessionEvents(sessionId)
  }

  const handleSendMessage = async (sessionId: string, content: string) => {
    await backend.sendPrompt(sessionId, content)
  }

  /** Export a conversation's events as a downloadable JSON file. */
  const handleExportSession = (sessionId: string) => {
    const session = backend.sessions.find((s) => s.id === sessionId)
    const events = (backend.events as AppEvent[]).filter((e) => e.sessionId === sessionId)
    const payload = {
      sessionId,
      name: session?.name ?? sessionId,
      agentId: session?.agentId,
      modelId: session?.modelId,
      exportedAt: new Date().toISOString(),
      events,
    }
    const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    const safeName = (session?.name ?? sessionId).replace(/[^a-z0-9_-]/gi, '_')
    a.download = `${safeName}.json`
    a.click()
    URL.revokeObjectURL(url)
  }

  // ---- Computed values ----
  const sessionEvents = activeSessionId
    ? (backend.events as AppEvent[]).filter((e) => e.sessionId === activeSessionId)
    : []

  // ---- Determine panel visibility based on viewport and state ----
  // On desktop, the sidebar can be hidden via Ctrl+B (width set to 0).
  const sidebarHidden = isDesktop && leftPanelWidth === 0
  const showLeftSidebar = (isDesktop && !sidebarHidden) || mobileView === 'explorer'
  const showEditor = isDesktop || mobileView === 'editor'
  const showChat = isDesktop || mobileView === 'chat'
  const showMobileSettings = !isDesktop && mobileView === 'settings'

  /**
   * Begins a panel-resize drag. Attaches window-level mousemove/mouseup
   * listeners that update the width state and tear themselves down on
   * release. The drag origin (startX/startWidth) is captured in the
   * listener closures — no React state or refs are touched during the
   * drag except the width setters (Feature 2).
   */
  const startPanelDrag = (side: 'left' | 'right') => (e: React.MouseEvent) => {
    e.preventDefault()
    const startX = e.clientX
    const startWidth = side === 'left' ? leftPanelWidth : rightPanelWidth

    const onMove = (ev: MouseEvent) => {
      const delta = ev.clientX - startX
      if (side === 'left') {
        // Dragging the right edge rightward widens the left panel.
        const next = Math.min(LEFT_MAX, Math.max(LEFT_MIN, startWidth + delta))
        setLeftPanelWidth(next)
      } else {
        // Dragging the left edge leftward widens the right panel.
        const next = Math.min(RIGHT_MAX, Math.max(RIGHT_MIN, startWidth - delta))
        setRightPanelWidth(next)
      }
    }
    const onUp = () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
    // Lock cursor + selection so dragging feels smooth across the whole window.
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
  }

  return (
    <div className="h-screen w-screen overflow-hidden flex flex-col bg-background text-gray-200 font-sans selection:bg-blue-500/30">
      {/* Reconnecting banner — shown only after a prior successful connection
          drops (mid-session Wi-Fi loss). A cold-load failure does not set
          reconnecting, so the banner stays hidden on first load. The pulsing
          dot uses the animate-pulse utility; semantic tokens only. */}
      {backend.reconnecting && (
        <div className="flex items-center gap-2 px-3 py-1.5 text-xs bg-muted text-muted-foreground border-b border-border shrink-0">
          <span className="inline-block h-2 w-2 rounded-full bg-muted-foreground/70 animate-pulse" />
          Reconnecting…
        </div>
      )}
      {/* Save error banner — transient, dismissible. Shown when a save fails
          so the error isn't silent (previously only console.error'd). */}
      {saveError && (
        <div className="flex items-center justify-between gap-2 px-3 py-1.5 text-xs bg-destructive/10 text-destructive border-b border-destructive/20 shrink-0">
          <span className="truncate">Save failed: {saveError}</span>
          <button
            type="button"
            onClick={() => setSaveError(null)}
            className="shrink-0 text-destructive/70 hover:text-destructive transition"
            aria-label="Dismiss save error"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      )}
      {/* Main app shell — activity bar + sidebar + editor + chat (horizontal) */}
      <div className="flex-1 min-h-0 flex">
      {/* Activity Bar (far left, icon-only — desktop only) */}
      <ActivityBar
        activePanel={leftPanel}
        onSwitchPanel={setLeftPanel}
        onOpenSettings={() => {
          if (isDesktop) setIsSettingsModalOpen(true)
          else setMobileView('settings')
        }}
      />

      {/* Left Sidebar — workspace switcher + file tree / search */}
      <LeftSidebar
        activePanel={leftPanel}
        onSwitchPanel={setLeftPanel}
        fileTree={fileTree}
        visible={showLeftSidebar}
        onFileSelect={handleFileSelect}
        workspaces={backend.workspaces}
        activeWorkspace={backend.activeWorkspace}
        onWorkspaceSelect={backend.selectWorkspace}
        onSearchResultSelect={handleSearchResultSelect}
        style={isDesktop ? { width: leftPanelWidth } : undefined}
      />

      {/* Resize handle between left sidebar and editor (desktop only) */}
      {isDesktop && showLeftSidebar && (
        <div
          onMouseDown={startPanelDrag('left')}
          className="w-1 cursor-col-resize bg-transparent hover:bg-accent-foreground/20 transition-colors shrink-0"
          title="Drag to resize"
        />
      )}

      {/* Center Editor — tabbed CodeMirror 6 with status bar */}
      <EditorPane
        tabs={openTabs}
        activeTabId={activeTabId}
        visible={showEditor}
        onTabSelect={handleTabSelect}
        onTabClose={handleTabClose}
        onSave={handleSave}
        onContentChange={handleContentChange}
        scrollToLine={searchResultLine}
      />

      {/* Resize handle between editor and right chat panel (desktop only) */}
      {isDesktop && showChat && (
        <div
          onMouseDown={startPanelDrag('right')}
          className="w-1 cursor-col-resize bg-transparent hover:bg-accent-foreground/20 transition-colors shrink-0"
          title="Drag to resize"
        />
      )}

      {/* Right Sidebar — agent chat */}
      <ChatPanel
        events={sessionEvents}
        agents={backend.agents}
        sessions={backend.sessions.map((s) => ({
          id: s.id,
          name: s.name,
          time: '',
          status: narrowStatus(s.status),
          active: s.id === activeSessionId,
          agentId: s.agentId,
          modelId: s.modelId,
          workspace: s.workspace,
        }))}
        workspaces={backend.workspaces}
        visible={showChat}
        connected={backend.connected}
        pendingPermissions={backend.pendingPermissions}
        activeSessionId={activeSessionId}
        onSendMessage={handleSendMessage}
        onCreateSession={handleCreateSession}
        onPermissionResponse={(requestId, decision) =>
          backend.respondPermission(requestId, decision)
        }
        onSelectSession={handleSelectSession}
        onCancel={(id) => backend.cancelSession(id)}
        onRenameSession={(id, name) => backend.renameSession(id, name)}
        onDeleteSession={(id) => backend.deleteSession(id)}
        onRebindSession={(id, agentId, modelId, maxTransferBytes) => backend.rebindSession(id, agentId, modelId, maxTransferBytes)}
        onExportSession={handleExportSession}
        style={isDesktop ? { width: rightPanelWidth } : undefined}
      />

      {/* Mobile Settings Panel (full-screen overlay) */}
      <MobileSettings
        devices={backend.devices.map((d) => ({
          id: d.id,
          name: d.name,
          icon: 'monitor',
          pairedAt: d.pairedAt,
        }))}
        agents={backend.agents}
        visible={showMobileSettings}
        onRevokeDevice={(id) => backend.revokeDevice(id)}
        onAutodetectAgents={async () => {
          const detected = await backend.autodetectAgents()
          for (const d of detected) {
            const existing = backend.agents.find(a => a.id === d.id)
            if (!existing) {
              await backend.addAgent(d)
            } else {
              await backend.addAgent({ ...existing, models: d.models, command: d.command })
            }
          }
        }}
      />

      </div>

      {/* Mobile Bottom Nav (hidden on desktop) */}
      <MobileNav activeView={mobileView} onSwitchView={setMobileView} />

      {/* Desktop Settings Modal */}
      <SettingsModal
        isOpen={isSettingsModalOpen}
        onClose={() => setIsSettingsModalOpen(false)}
        agents={backend.agents}
        onAddAgent={backend.addAgent}
        onDeleteAgent={backend.deleteAgent}
        onAutodetect={backend.autodetectAgents}
      />
    </div>
  )
}
