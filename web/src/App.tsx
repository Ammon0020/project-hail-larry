import { useState, useEffect } from 'react'
import { LockScreen } from '@/components/LockScreen'
import { ActivityBar } from '@/components/ActivityBar'
import { LeftSidebar } from '@/components/LeftSidebar'
import { EditorPane } from '@/components/EditorPane'
import { ChatPanel } from '@/components/ChatPanel'
import { MobileNav } from '@/components/MobileNav'
import { MobileSettings } from '@/components/MobileSettings'
import { SettingsModal } from '@/components/SettingsModal'
import { useBackend } from '@/hooks/useBackend'
import type { LeftPanel, MobileView, FileTreeNode, AppEvent, Session, Tab } from '@/types'

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
    return !!localStorage.getItem('deviceCredential')
  })

  // Panel state — restored from localStorage so the layout survives a reload.
  const [leftPanel, setLeftPanel] = useState<LeftPanel>(
    () => (localStorage.getItem('lai:leftPanel') as LeftPanel) || 'files',
  )
  const [mobileView, setMobileView] = useState<MobileView>(
    () => (localStorage.getItem('lai:mobileView') as MobileView) || 'editor',
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

  // Session state — restored from localStorage so the active conversation
  // survives a page reload (UI Spec §6.2).
  const [activeSessionId, setActiveSessionId] = useState<string | null>(
    () => localStorage.getItem('activeSessionId') || null,
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
    if (activeSessionId) localStorage.setItem('activeSessionId', activeSessionId)
    else localStorage.removeItem('activeSessionId')
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

  // ---- Lock screen for unpaired devices ----
  if (!paired) {
    return <LockScreen onPaired={() => setPaired(true)} />
  }

  // Convert backend file tree to the component's expected format, preserving path.
  const convertNode = (n: { name: string; type: string; path?: string; children?: { name: string; type: string; path?: string; children?: unknown[] }[] }): FileTreeNode => ({
    name: n.name,
    type: n.type as 'folder' | 'file',
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

  // ---- Tab operations ----
  const handleTabSelect = (id: string) => setActiveTabId(id)

  const handleTabClose = (id: string) => {
    setOpenTabs((prev) => {
      const next = prev.filter((t) => t.id !== id)
      if (activeTabId === id) {
        setActiveTabId(next.length > 0 ? next[next.length - 1].id : null)
      }
      return next
    })
  }

  const handleContentChange = (content: string) => {
    setOpenTabs((prev) =>
      prev.map((t) => (t.id === activeTabId ? { ...t, content, unsaved: true } : t)),
    )
  }

  const handleSave = async () => {
    const tab = openTabs.find((t) => t.id === activeTabId)
    if (!tab) return
    try {
      const result = await backend.saveFile(tab.path, tab.content, tab.revision)
      setOpenTabs((prev) =>
        prev.map((t) =>
          t.id === activeTabId ? { ...t, revision: result.revision, unsaved: false } : t,
        ),
      )
    } catch (err) {
      console.error('Save failed:', err)
    }
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
    // Switching conversations is a pure filter change over the master event
    // list: ws.onmessage appends ALL events for ALL sessions to eventsRef
    // regardless of which conversation is active, so events that arrived for
    // session A while the user was viewing session B are already present and
    // surface immediately when we filter back to A.
    //
    // We deliberately do NOT call backend.loadSessionEvents() here. That fetch
    // raced with WebSocket delivery: it replaced/merged eventsRef for the
    // session against a SQLite snapshot, and any StreamUpdate events not yet
    // persisted at fetch time (or delivered between fetch-initiation and
    // completion) were dropped — freezing the streamed response at the point
    // the user switched away. History for a session is loaded once on initial
    // page load (loadedEventsForSession below) and caught up on WebSocket
    // reconnect (loadEvents merges by cursor); a page refresh restores full
    // history if the socket was down long enough to miss events.
    setActiveSessionId(sessionId)
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
  const showLeftSidebar = isDesktop || mobileView === 'explorer'
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
    <div className="h-screen w-screen overflow-hidden flex bg-background text-gray-200 font-sans selection:bg-blue-500/30">
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
          status: s.status as Session['status'],
          active: s.id === activeSessionId,
          agentId: s.agentId,
          modelId: s.modelId,
        }))}
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
        onRebindSession={(id, agentId, modelId) => backend.rebindSession(id, agentId, modelId)}
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
