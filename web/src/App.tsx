import { useState, useEffect, useCallback, useRef } from 'react'
import { X, Wifi, WifiOff, Bot } from 'lucide-react'
import { LockScreen } from '@/components/LockScreen'
import { ActivityBar } from '@/components/ActivityBar'
import { LeftSidebar } from '@/components/LeftSidebar'
import { EditorPane } from '@/components/EditorPane'
import { TabBar } from '@/components/TabBar'
import { WorkspaceHeader } from '@/components/WorkspaceHeader'
import { Banner } from '@/components/ui/Banner'
import { cn } from '@/lib/utils'
import { ChatPanel } from '@/components/ChatPanel'
import { MobileNav } from '@/components/MobileNav'
import { StatusBar } from '@/components/StatusBar'
import { useBackend } from '@/hooks/useBackend'
import { useFileChangeDetection } from '@/hooks/useFileChangeDetection'
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'
import { usePanelResize } from '@/hooks/usePanelResize'
import type { EditorSelectionInfo } from '@/lib/api'
import type { LeftPanel, MobileView, FileTreeNode, AppEvent, Attachment, SessionStatus, Tab } from '@/types'

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
    () => readValidString('lai:mobileView', ['explorer', 'editor', 'chat'] as const, 'editor'),
  )
  const [isDesktop, setIsDesktop] = useState(
    typeof window !== 'undefined' ? window.innerWidth >= 1024 : true,
  )

  // Resizable panel widths — restored from localStorage so the layout
  // survives a reload (Feature 2). Defaults: 260px left, 420px right.
  const LEFT_MIN = 180
  const LEFT_MAX = 480
  const RIGHT_MIN = 300
  const RIGHT_MAX = 700
  const {
    leftWidth: leftPanelWidth,
    rightWidth: rightPanelWidth,
    setLeftWidth: setLeftPanelWidth,
    setRightWidth: setRightPanelWidth,
    startLeftDrag,
    startRightDrag,
    hideLeftPanel,
    showLeftPanel,
    toggleLeftPanel,
    toggleRightPanel,
  } = usePanelResize({
    left: {
      initialWidth: 260,
      minWidth: LEFT_MIN,
      maxWidth: LEFT_MAX,
      storageKey: 'lai:leftPanelWidth',
    },
    right: {
      initialWidth: 420,
      minWidth: RIGHT_MIN,
      maxWidth: RIGHT_MAX,
      storageKey: 'lai:rightPanelWidth',
    },
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
  const lastCodeTabIdRef = useRef<string | null>(
    activeTabId !== 'settings' ? activeTabId : null
  )

  useEffect(() => {
    if (activeTabId && activeTabId !== 'settings') {
      lastCodeTabIdRef.current = activeTabId
    }
  }, [activeTabId])

  const [wrap, setWrap] = useState(false)

  // Editor font size — persisted to localStorage so the user's zoom preference
  // survives reloads. Lifted from EditorPane so the StatusBar (rendered at the
  // app level on desktop, spanning the full width) can read and adjust it.
  // Defaults to 13px on desktop, 15px on mobile for readability on small screens.
  const [fontSize, setFontSize] = useState<number>(() => {
    const stored = localStorage.getItem('lai:editor-font-size')
    if (stored) return parseInt(stored, 10) || (isDesktop ? 13 : 15)
    return isDesktop ? 13 : 15
  })
  useEffect(() => {
    localStorage.setItem('lai:editor-font-size', String(fontSize))
  }, [fontSize])

  // Current editor selection, lifted from EditorPane via the
  // onSelectionChange callback so it can be reported to the backend alongside
  // open files (ACP spec item 1.3 — selection sent as a resource block).
  // undefined means "no active selection".
  const [editorSelection, setEditorSelection] = useState<EditorSelectionInfo | undefined>(undefined)

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
  // Pull the stable action methods used as effect/callback dependencies out of
  // the backend object so the react-hooks/exhaustive-deps rule sees standalone
  // stable identifiers (the backend object literal is new each render, but the
  // action identities it carries are now stable via useCallback in useBackend).
  const { loadSessionEvents, reportContext, saveFile, readFile } = backend

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
  // After a daemon restart the session may no longer exist (conversations.json
  // wiped, or the session was deleted). If the loaded list is non-empty and
  // does not contain the active id, clear it so the UI shows the new-chat state
  // instead of sending prompts to a dead session. Uses the "adjust state during
  // render" pattern (React docs) — this is a PURE state adjustment (no data
  // fetch / side effect), so it neither runs a side effect during render nor
  // trips react-hooks/set-state-in-effect. The empty-list case is handled
  // defensively by sendPrompt's 404 path in useBackend.
  const [prevSessions, setPrevSessions] = useState(backend.sessions)
  if (backend.sessions !== prevSessions) {
    setPrevSessions(backend.sessions)
    if (
      activeSessionId &&
      backend.sessions.length > 0 &&
      !backend.sessions.some((s) => s.id === activeSessionId)
    ) {
      setActiveSessionId(null)
    }
  }

  // Tracks the session id whose events we've already loaded so each session's
  // persisted history is fetched at most once. A ref (not state) so updating it
  // inside the effect below doesn't trip react-hooks/set-state-in-effect and
  // doesn't trigger a re-render. handleCreateSession / handleSelectSession
  // pre-set it so the effect only handles the cold-reload restore path.
  const loadedSessionRef = useRef<string | null>(null)

  // On a cold reload, fetch the restored conversation's history exactly once.
  // The DATA FETCH lives in a guarded effect (not a render-time side effect) so
  // it never runs during render (fixes web-app-side-effect-during-render). On a
  // page reload localStorage restores activeSessionId, but the global
  // loadEvents() only fetched the first slice across all sessions, so the active
  // conversation's history may be missing — fetch it explicitly here.
  //
  // Brand-new sessions can't reach the fetch because handleCreateSession
  // pre-marks them loaded — preventing the fetch from racing the in-flight
  // prompt POST (that race made the user's first message flash then vanish).
  // Stale sessions are cleared by the render-time block above, so the effect
  // simply skips them. The loadedSessionRef guard ensures each session's
  // history is fetched at most once, so a stable loadSessionEvents identity
  // in the deps cannot cause redundant fetches.
  useEffect(() => {
    if (!activeSessionId) {
      loadedSessionRef.current = null
      return
    }
    if (backend.sessions.length === 0) return
    if (!backend.sessions.some((s) => s.id === activeSessionId)) return
    if (loadedSessionRef.current !== activeSessionId) {
      loadedSessionRef.current = activeSessionId
      loadSessionEvents(activeSessionId)
    }
  }, [backend.sessions, activeSessionId, loadSessionEvents])

  /** Persist open tabs, active tab, panel, and mobile view so the layout
   *  survives a page reload (UI Spec §6.2 — UI Persistence). */
  useEffect(() => {
    try {
      // Settings tabs are not persisted — they're synthetic, not files.
      // Preview tabs are transient and not persisted either.
      const persistable = openTabs.filter((t) => t.kind !== 'settings' && !t.isPreview)
      localStorage.setItem('lai:openTabs', JSON.stringify(persistable))
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
  // Skipped when there's no active session or no active workspace. The current
  // editor selection is included so the backend can emit it as a resource block
  // (ACP spec item 1.3).
  useEffect(() => {
    if (!activeSessionId || !backend.activeWorkspace) return
    const openFiles = openTabs.map((t) => t.path)
    const recentEdits = openTabs.filter((t) => t.unsaved).map((t) => t.path)
    reportContext(activeSessionId, openFiles, recentEdits, editorSelection)
  }, [openTabs, activeSessionId, editorSelection, backend.activeWorkspace, reportContext])

  useEffect(() => {
    localStorage.setItem('lai:leftPanel', leftPanel)
  }, [leftPanel])

  useEffect(() => {
    localStorage.setItem('lai:mobileView', mobileView)
  }, [mobileView])

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

  useFileChangeDetection(backend, openTabs, setOpenTabs)

  // ---- Tab operations ----
  // Defined before the unpaired early return so the keyboard-shortcut
  // useEffect below them is not called conditionally (react-hooks/rules-of-hooks).
  const handleTabSelect = (id: string) => {
    setActiveTabId(id)
    setOpenTabs((prev) =>
      prev.map((t) => (t.id === id && t.isPreview ? { ...t, isPreview: false } : t)),
    )
  }

  /** Opens the settings tab (singleton id 'settings'). If already open,
   *  activates it; otherwise creates and activates it. Settings tabs are
   *  not persisted to localStorage (filtered out in the persistence effect). */
  const openSettingsTab = () => {
    setOpenTabs((prev) => {
      if (prev.some((t) => t.id === 'settings')) return prev
      return [...prev, {
        id: 'settings',
        name: 'Settings',
        path: 'settings',
        content: '',
        revision: 0,
        unsaved: false,
        language: '',
        kind: 'settings' as const,
      }]
    })
    setActiveTabId('settings')
  }

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

  /** Close every tab except the given one (settings tabs are always kept). */
  const handleCloseOthers = useCallback(
    (id: string) => {
      setOpenTabs((prev) => {
        const next = prev.filter((t) => t.id === id || t.kind === 'settings')
        if (activeTabId && !next.some((t) => t.id === activeTabId)) {
          setActiveTabId(next.length > 0 ? next[next.length - 1].id : null)
        }
        return next
      })
    },
    [activeTabId],
  )

  /** Close all saved (non-unsaved) tabs except the given one. Settings tabs
   *  are always kept. */
  const handleCloseSaved = useCallback(
    (id: string) => {
      setOpenTabs((prev) => {
        const next = prev.filter((t) => t.unsaved || t.id === id || t.kind === 'settings')
        if (activeTabId && !next.some((t) => t.id === activeTabId)) {
          setActiveTabId(next.length > 0 ? next[next.length - 1].id : null)
        }
        return next
      })
    },
    [activeTabId],
  )

  /** Close all tabs to the right of the given tab (settings tabs are kept). */
  const handleCloseToRight = useCallback(
    (id: string) => {
      setOpenTabs((prev) => {
        const idx = prev.findIndex((t) => t.id === id)
        if (idx === -1) return prev
        const next = prev.filter((t, i) => i <= idx || t.kind === 'settings')
        if (activeTabId && !next.some((t) => t.id === activeTabId)) {
          setActiveTabId(next.length > 0 ? next[next.length - 1].id : null)
        }
        return next
      })
    },
    [activeTabId],
  )

  const handleCopyPath = useCallback((path: string) => {
    navigator.clipboard?.writeText(path).catch(() => {})
  }, [])

  const handleKeepOpen = useCallback((id: string) => {
    setOpenTabs((prev) => prev.map((t) => (t.id === id ? { ...t, isPreview: false } : t)))
  }, [])

  const handleContentChange = (content: string) => {
    setOpenTabs((prev) =>
      prev.map((t) =>
        t.id === activeTabId ? { ...t, content, unsaved: true, isPreview: false } : t,
      ),
    )
  }

  const handleSave = useCallback(async () => {
    const tab = openTabs.find((t) => t.id === activeTabId)
    if (!tab) return
    try {
      const result = await saveFile(tab.path, tab.content, tab.revision, tab.workspaceId)
      setOpenTabs((prev) =>
        prev.map((t) =>
          t.id === activeTabId
            ? { ...t, revision: result.revision, unsaved: false, changedOnDisk: false }
            : t,
        ),
      )
      setSaveError(null)
    } catch (err) {
      console.error('Save failed:', err)
      setSaveError(err instanceof Error ? err.message : String(err))
    }
  }, [saveFile, openTabs, activeTabId])

  /** Reloads a tab's content from disk, discarding local edits. Invoked from
   *  the EditorPane "changed on disk" banner's Reload action. */
  const handleReloadTab = useCallback(
    async (tabId: string) => {
      const tab = openTabs.find((t) => t.id === tabId)
      if (!tab) return
      try {
        const file = await readFile(tab.path, tab.workspaceId)
        setOpenTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  content: file.content,
                  revision: file.revision,
                  isBinary: file.isBinary ?? false,
                  previewable: file.previewable ?? false,
                  unsaved: false,
                  changedOnDisk: false,
                }
              : t,
          ),
        )
        setSaveError(null)
      } catch (err) {
        setSaveError(err instanceof Error ? err.message : String(err))
      }
    },
    [readFile, openTabs],
  )

  /** Toggles a text-preview tab between edit (CodeMirror) and preview
   *  (FileViewer) modes. Only applies to files with previewable=true and
   *  isBinary=false — binary files always show FileViewer. */
  const handleToggleViewMode = useCallback((tabId: string) => {
    setOpenTabs((prev) =>
      prev.map((t) =>
        t.id === tabId && t.previewable && !t.isBinary
          ? { ...t, viewMode: t.viewMode === 'preview' ? 'edit' : 'preview' }
          : t,
      ),
    )
  }, [])

  // ---- Global keyboard shortcuts ----
  // Registered on window so they work even when the CodeMirror editor isn't
  // focused. Ctrl+S is also handled inside CodeMirror (Prec.highest) for when
  // the editor IS focused — this is the fallback for when it isn't.
  useKeyboardShortcuts((e) => {
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
      toggleLeftPanel()
    }
  })

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
      if (!isDesktop) setMobileView('editor')
      return
    }
    // Load file from backend
    try {
      const file = await backend.readFile(path)
      const name = path.split(/[\\/]/).pop() || path
      const ext = name.split('.').pop() || ''
      const tab: Tab = {
        id: path,
        name,
        path,
        content: file.content,
        revision: file.revision,
        unsaved: false,
        language: ext.toLowerCase(),
        isBinary: file.isBinary ?? false,
        previewable: file.previewable ?? false,
        workspaceId: backend.activeWorkspace?.id,
        isPreview: true,
      }
      setOpenTabs((prev) => {
        const previewIdx = prev.findIndex((t) => t.isPreview)
        if (previewIdx === -1) return [...prev, tab]
        const next = prev.slice()
        next[previewIdx] = tab
        return next
      })
      setActiveTabId(path)
      if (!isDesktop) setMobileView('editor')
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
    // Mark the new session as already-loaded so the restore effect does NOT
    // fetch its (empty) history and race the in-flight first prompt POST.
    loadedSessionRef.current = session.id
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
    //
    // Mark it loaded so the restore effect treats this explicit fetch as the
    // one-time load and does not fetch again for the same session.
    loadedSessionRef.current = sessionId
    setActiveSessionId(sessionId)
    backend.loadSessionEvents(sessionId)
  }

  const handleSendMessage = async (
    sessionId: string,
    content: string,
    attachments?: Attachment[],
  ) => {
    await backend.sendPrompt(sessionId, content, attachments)
  }

  /** Export a conversation as a downloadable markdown transcript. The backend
   *  renders the full event history into a readable transcript; this just
   *  triggers the download via the api client. */
  const handleExportSession = async (sessionId: string) => {
    await backend.exportSession(sessionId)
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
  const showChat = (isDesktop && rightPanelWidth > 0) || mobileView === 'chat'

  return (
    <div className="h-screen w-screen overflow-hidden flex flex-col bg-background text-foreground font-sans selection:bg-primary/30">
      {/* Reconnecting banner — shown only after a prior successful connection
          drops (mid-session Wi-Fi loss). A cold-load failure does not set
          reconnecting, so the banner stays hidden on first load. The pulsing
          dot uses the animate-pulse utility; semantic tokens only. */}
      {backend.reconnecting && (
        <Banner
          variant="info"
          role="status"
          className="flex items-center gap-2 px-3 py-1.5 border-b shrink-0"
        >
          <span className="inline-block h-2 w-2 rounded-full bg-muted-foreground/70 animate-pulse" />
          Reconnecting…
        </Banner>
      )}
      {/* Save error banner — transient, dismissible. Shown when a save fails
          so the error isn't silent (previously only console.error'd). */}
      {saveError && (
        <Banner
          variant="error"
          role="alert"
          className="flex items-center justify-between gap-2 px-3 py-1.5 border-b border-destructive/20 shrink-0"
        >
          <span className="truncate">Save failed: {saveError}</span>
          <button
            type="button"
            onClick={() => setSaveError(null)}
            className="shrink-0 text-destructive/70 hover:text-destructive transition"
            aria-label="Dismiss save error"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </Banner>
      )}
      {/* Top Header Bar (Desktop only) */}
      {isDesktop && (
        <div className="min-h-[36px] border-b border-border flex items-stretch bg-panel shrink-0 select-none">
          {/* Left section: width matches ActivityBar + LeftSidebar */}
          <div
            className="flex items-center justify-between pl-2 pr-3 border-r border-border shrink-0"
            style={{ width: leftPanelWidth + 48 }}
          >
            <div className={cn("flex w-full", leftPanelWidth === 0 ? "justify-center" : "justify-start")}>
              {leftPanelWidth === 0 ? (
                <div
                  className={cn(
                    'flex items-center justify-center w-5 h-5 rounded-full border border-none bg-transparent p-0 shrink-0 cursor-default',
                    backend.connected ? 'text-green-400' : 'text-muted-foreground'
                  )}
                  title={backend.connected ? 'Connected to backend' : 'Backend offline — reconnecting…'}
                >
                  {backend.connected ? <Wifi className="w-4 h-4" /> : <WifiOff className="w-4 h-4 animate-pulse" />}
                </div>
              ) : (
                <WorkspaceHeader 
                  connected={backend.connected} 
                  workspaces={backend.workspaces} 
                  activeWorkspace={backend.activeWorkspace} 
                  onWorkspaceSelect={backend.selectWorkspace} 
                />
              )}
            </div>
          </div>

          {/* Right section: Tabs to the right */}
          <div className="flex-1 flex items-stretch min-w-0 h-full @container">
            <div className="flex-1 min-w-0 flex items-stretch">
              <TabBar
                tabs={openTabs}
                activeTabId={activeTabId}
                onTabSelect={handleTabSelect}
                onTabClose={handleTabClose}
                onSave={handleSave}
                wrap={wrap}
                onToggleWrap={() => setWrap(!wrap)}
                onCloseOthers={handleCloseOthers}
                onCloseSaved={handleCloseSaved}
                onCloseToRight={handleCloseToRight}
                onCopyPath={handleCopyPath}
                onCopyRelativePath={handleCopyPath}
                onKeepOpen={handleKeepOpen}
              />
            </div>
            <button
              type="button"
              title={rightPanelWidth > 0 ? "Hide chat panel" : "Show chat panel"}
              aria-label="Toggle chat panel"
              aria-pressed={rightPanelWidth > 0}
              onClick={toggleRightPanel}
              className={cn(
                "shrink-0 self-center mr-2 w-7 h-6 rounded flex items-center justify-center transition",
                rightPanelWidth > 0
                  ? "bg-primary text-primary-foreground hover:bg-primary/90"
                  : "bg-secondary text-secondary-foreground hover:bg-accent",
              )}
            >
              <Bot className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      )}

      {/* Main app shell — activity bar + sidebar + editor + chat (horizontal) */}
      <div className="flex-1 min-h-0 flex">
      {/* Activity Bar (far left, icon-only — desktop only) */}
      <ActivityBar
        activePanel={leftPanel}
        // VS Code-style toggle: open when closed, close when clicking the active icon.
        onSwitchPanel={(id) => {
          if (leftPanelWidth === 0) {
            // Sidebar is hidden — open it with the clicked panel.
            setLeftPanel(id)
            showLeftPanel()
          } else if (leftPanel === id) {
            // Sidebar is open and clicking the already-active panel — close it (VS Code toggle).
            hideLeftPanel()
          } else {
            // Sidebar is open and clicking a different panel — just switch.
            setLeftPanel(id)
          }
        }}
        onOpenSettings={() => {
          openSettingsTab()
          if (!isDesktop) setMobileView('editor')
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
        connected={backend.connected}
      />

      {/* Resize handle between left sidebar and editor (desktop only) */}
      {isDesktop && showLeftSidebar && (
        <div
          role="separator"
          tabIndex={0}
          aria-orientation="vertical"
          aria-valuenow={leftPanelWidth}
          aria-valuemin={LEFT_MIN}
          aria-valuemax={LEFT_MAX}
          onMouseDown={startLeftDrag}
          onKeyDown={(e) => {
            if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
            e.preventDefault()
            const step = e.key === 'ArrowRight' ? 16 : -16
            setLeftPanelWidth((w) => Math.min(LEFT_MAX, Math.max(LEFT_MIN, w + step)))
          }}
          className="w-1 cursor-col-resize bg-transparent hover:bg-accent-foreground/20 focus:outline-none focus-visible:bg-accent-foreground/30 transition-colors shrink-0"
          title="Drag to resize"
        />
      )}

      <EditorPane
        tabs={openTabs}
        activeTabId={activeTabId}
        visible={showEditor}
        onTabSelect={handleTabSelect}
        onTabClose={handleTabClose}
        onSave={handleSave}
        onContentChange={handleContentChange}
        onReloadTab={handleReloadTab}
        onSelectionChange={setEditorSelection}
        scrollToLine={searchResultLine}
        settingsProps={{
          agents: backend.agents,
          onAddAgent: backend.addAgent,
          onDeleteAgent: backend.deleteAgent,
          onAutodetect: backend.autodetectAgents,
          activeSessionId,
        }}
        hideTabBar={isDesktop}
        wrap={wrap}
        onToggleWrap={() => setWrap(!wrap)}
        onToggleViewMode={handleToggleViewMode}
        isDesktop={isDesktop}
        workspaceName={backend.activeWorkspace?.name}
        onCloseOthers={handleCloseOthers}
        onCloseSaved={handleCloseSaved}
        onCloseToRight={handleCloseToRight}
        onCopyPath={handleCopyPath}
        onCopyRelativePath={handleCopyPath}
        onKeepOpen={handleKeepOpen}
        fontSize={fontSize}
        onFontSizeChange={setFontSize}
      />

      {/* Resize handle between editor and right chat panel (desktop only) */}
      {isDesktop && showChat && (
        <div
          role="separator"
          tabIndex={0}
          aria-orientation="vertical"
          aria-valuenow={rightPanelWidth}
          aria-valuemin={RIGHT_MIN}
          aria-valuemax={RIGHT_MAX}
          onMouseDown={startRightDrag}
          onKeyDown={(e) => {
            if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
            e.preventDefault()
            // Inverted vs. the left handle: ArrowLeft widens the right panel,
            // matching the mouse drag direction (drag left edge leftward).
            const step = e.key === 'ArrowLeft' ? 16 : -16
            setRightPanelWidth((w) => Math.min(RIGHT_MAX, Math.max(RIGHT_MIN, w + step)))
          }}
          className="w-1 cursor-col-resize bg-transparent hover:bg-accent-foreground/20 focus:outline-none focus-visible:bg-accent-foreground/30 transition-colors shrink-0"
          title="Drag to resize"
        />
      )}

      {/* Right Sidebar — agent chat */}
      <ChatPanel
        events={sessionEvents}
        allEvents={backend.events as AppEvent[]}
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
        workspaceId={backend.activeWorkspace?.id ?? ''}
        visible={showChat}
        connected={backend.connected}
        isDesktop={isDesktop}
        pendingPermissions={backend.pendingPermissions}
        activeSessionId={activeSessionId}
        actions={{
          onSendMessage: handleSendMessage,
          onCreateSession: handleCreateSession,
          onPermissionResponse: backend.respondPermission,
          onSelectSession: handleSelectSession,
          onCancel: backend.cancelSession,
          onRenameSession: backend.renameSession,
          onDeleteSession: backend.deleteSession,
          onRebindSession: backend.rebindSession,
          onSwitchModel: backend.switchModel,
          onExportSession: handleExportSession,
          onUploadFile: backend.uploadFile,
          onSelectWorkspace: (id) => {
            const workspace = backend.workspaces.find((candidate) => candidate.id === id)
            if (workspace) backend.selectWorkspace(workspace)
          },
        }}
        style={isDesktop ? { width: rightPanelWidth } : undefined}
      />

      </div>

      {/* Status Bar (desktop only) — spans the full app width, sitting below the
       *  editor + chat shell and above the (desktop-hidden) mobile nav. On
       *  mobile the StatusBar is rendered inside EditorPane instead. */}
      {isDesktop && (
        <StatusBar
          activeTab={openTabs.find((t) => t.id === activeTabId) || null}
          fontSize={fontSize}
          onFontSizeChange={setFontSize}
        />
      )}

      {/* Mobile Bottom Nav (hidden on desktop) */}
      <MobileNav
        activeView={mobileView}
        onSwitchView={(v) => {
          if (v === 'editor' && activeTabId === 'settings') {
            const lastCodeExists = openTabs.some(t => t.id === lastCodeTabIdRef.current)
            if (lastCodeExists && lastCodeTabIdRef.current) {
              setActiveTabId(lastCodeTabIdRef.current)
            } else {
              const firstCodeTab = openTabs.find(t => t.kind !== 'settings')
              setActiveTabId(firstCodeTab ? firstCodeTab.id : null)
            }
          }
          setMobileView(v)
        }}
        onOpenSettings={() => {
          openSettingsTab()
          setMobileView('editor')
        }}
        settingsActive={!isDesktop && mobileView === 'editor' && activeTabId === 'settings'}
      />
    </div>
  )
}
