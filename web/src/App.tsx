import { useState, useEffect, useCallback, useRef, useMemo } from 'react'
import { X, Wifi, WifiOff, Bot, Command, PanelLeft, FolderOpen, GitBranch, Search, Settings, Save } from 'lucide-react'
import { useEditorSettings } from '@/hooks/useEditorSettings'
import { LockScreen } from '@/components/LockScreen'
import { CommandPalette, type Command as PaletteCommand } from '@/components/CommandPalette'
import { ActivityBar } from '@/components/ActivityBar'
import { LeftSidebar } from '@/components/LeftSidebar'
import { EditorPane } from '@/components/EditorPane'
import { TabBar } from '@/components/TabBar'
import { WorkspaceHeader } from '@/components/WorkspaceHeader'
import { Banner } from '@/components/ui/Banner'
import { joinWorkspacePath } from '@/lib/tabPath'
import { cn } from '@/lib/utils'
import { ChatPanel } from '@/components/ChatPanel'
import { ErrorBoundary } from '@/components/ErrorBoundary'
import { MobileNav } from '@/components/MobileNav'
import { StatusBar } from '@/components/StatusBar'
import { useBackend } from '@/hooks/useBackend'
import { useTabManager } from '@/hooks/useTabManager'
import { useEditorTabHandlers } from '@/hooks/useEditorTabHandlers'
import { useGitState } from '@/hooks/useGitState'
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'
import { useLayoutState } from '@/hooks/useLayoutState'
import type { EditorSelectionInfo } from '@/lib/api'
import type { FileNode, FileTreeNode, AppEvent, Attachment, SessionStatus } from '@/types'

/**
 * Runtime guard that narrows an arbitrary backend status string to the
 * SessionStatus union. Falls back to 'created' for unknown values so a
 * malformed status never yields an undefined className lookup in
 * ChatHistory's statusDotClass map (AGENTS.md — type safety).
 */
const VALID_SESSION_STATUSES: readonly SessionStatus[] = [
  'created', 'idle', 'running', 'interrupted', 'failed', 'closed',
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
  // Pairing state — check sessionStorage for existing credential (Blueprint Sec 19).
  const [paired, setPaired] = useState(() => {
    return !!sessionStorage.getItem('lai:deviceCredential')
  })

  const {
    isDesktop,
    leftPanel,
    setLeftPanel,
    mobileView,
    setMobileView,
    settingsSection,
    setSettingsSection,
    leftPanelWidth,
    rightPanelWidth,
    setLeftPanelWidth,
    setRightPanelWidth,
    startLeftDrag,
    startRightDrag,
    hideLeftPanel,
    showLeftPanel,
    toggleLeftPanel,
    toggleRightPanel,
    LEFT_MIN,
    LEFT_MAX,
    RIGHT_MIN,
    RIGHT_MAX,
    showLeftSidebar,
    showEditor,
    showChat,
  } = useLayoutState(readValidString)

  // Editor preferences (font size, word wrap, tab size, line numbers, fold
  // gutter, bracket matching, auto indent, close brackets) — persisted to
  // localStorage via useEditorSettings. Font size and wrap are lifted here so
  // the StatusBar (desktop, full-width) and TabBar can read/adjust them.
  const { settings: editorSettings, update: updateEditorSettings, setFontSize, setWrap } = useEditorSettings(isDesktop)
  const { fontSize, wrap } = editorSettings

  // Current editor selection, lifted from EditorPane via the
  // onSelectionChange callback so it can be reported to the backend alongside
  // open files (ACP spec item 1.3 — selection sent as a resource block).
  // undefined means "no active selection".
  const [editorSelection, setEditorSelection] = useState<EditorSelectionInfo | undefined>(undefined)

  // Cursor (head) position lifted from EditorPane so the desktop StatusBar can
  // show a truthful Ln/Col readout. undefined until the editor reports it.
  const [cursorPos, setCursorPos] = useState<{ line: number; col: number } | undefined>(undefined)

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
  const {
    openTabs,
    setOpenTabs,
    activeTabId,
    setActiveTabId,
    lastCodeTabIdRef,
    fileSelectTokenRef,
    saveError,
    setSaveError,
    handleTabSelect,
    openSettingsTab,
    handleTabClose,
    handleCloseOthers,
    handleCloseSaved,
    handleCloseToRight,
    handleTreeRename,
    handleTreeDelete,
    handleKeepOpen,
    handleContentChange,
    handleSave,
    handleReloadTab,
    handleToggleViewMode,
  } = useTabManager({
    setSettingsSection,
    backend,
    saveFile,
    readFile,
    reportContext,
    editorSelection,
    activeSessionId,
  })
  const {
    searchResultLine,
    handleFileSelect,
    handleOpenPreview,
    handleOpenDiff,
    handleOpenCommitDiff,
    handleTreeNewFile,
    handleTreeNewFolder,
    handleSearchResultSelect,
  } = useEditorTabHandlers({
    openTabs,
    setOpenTabs,
    activeTabId,
    setActiveTabId,
    fileSelectTokenRef,
    handleToggleViewMode,
    isDesktop,
    setMobileView,
    backend,
  })

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
  // page reload localStorage restores activeSessionId, and the global tail is
  // shared across sessions, so fetch the active conversation explicitly here.
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

  const { gitState, refresh: refreshGitState } = useGitState(backend.activeWorkspace?.id)
  const gitBranch = gitState?.repoDetected ? gitState.headBranch : null

  const handleCopyPath = useCallback((path: string) => {
    const root = backend.activeWorkspace?.path
    const absolute = root ? joinWorkspacePath(root, path) : path
    navigator.clipboard?.writeText(absolute).catch(() => {})
  }, [backend.activeWorkspace?.path])

  const handleCopyRelativePath = useCallback((path: string) => {
    navigator.clipboard?.writeText(path).catch(() => {})
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

    if (mod && !e.shiftKey && (e.key === 'p' || e.key === 'P')) {
      e.preventDefault()
      window.dispatchEvent(new CustomEvent('command-palette-open'))
      return
    }
  })

  // Convert backend file tree to the component's expected format, preserving
  // path. Validates `type` at runtime so a malformed backend node cannot
  // silently become a typed union (AGENTS.md — type safety).
  const fileTree: FileTreeNode[] = useMemo(() => {
    const convertNode = (n: FileNode): FileTreeNode => ({
      name: n.name,
      type: n.type === 'folder' ? 'folder' : 'file',
      path: n.path,
      children: n.children?.map(convertNode),
    })
    return backend.fileTree.map(convertNode)
  }, [backend.fileTree])

  const commands: PaletteCommand[] = useMemo(() => [
    { id: 'toggle-sidebar', label: 'Toggle Sidebar', icon: <PanelLeft className="w-4 h-4" />, action: () => toggleLeftPanel() },
    { id: 'toggle-chat', label: 'Toggle Chat Panel', icon: <Bot className="w-4 h-4" />, action: () => toggleRightPanel() },
    { id: 'open-explorer', label: 'Open Explorer', icon: <FolderOpen className="w-4 h-4" />, action: () => { setLeftPanel('files'); showLeftPanel() } },
    { id: 'open-search', label: 'Open Search', icon: <Search className="w-4 h-4" />, action: () => { setLeftPanel('search'); showLeftPanel() } },
    { id: 'open-source-control', label: 'Open Source Control', icon: <GitBranch className="w-4 h-4" />, action: () => { setLeftPanel('git'); showLeftPanel() } },
    { id: 'open-settings', label: 'Open Settings', icon: <Settings className="w-4 h-4" />, action: () => openSettingsTab() },
    { id: 'save-file', label: 'Save File', icon: <Save className="w-4 h-4" />, action: () => handleSave() },
    { id: 'close-tab', label: 'Close Active Tab', icon: <X className="w-4 h-4" />, action: () => { if (activeTabId) handleTabClose(activeTabId) } },
  ], [activeTabId, toggleLeftPanel, toggleRightPanel, showLeftPanel, setLeftPanel, openSettingsTab, handleSave, handleTabClose])

  // ---- Lock screen for unpaired devices ----
  if (!paired) {
    return <LockScreen onPaired={() => setPaired(true)} />
  }

  // ---- Session operations ----
  const handleCreateSession = async (agentId: string, modelId: string, profileId?: string): Promise<string> => {
    const session = await backend.createSession(agentId, modelId, profileId)
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
    // initial loadEvents() retains only the global tail, which may not include
    // the selected conversation's history. So we explicitly fetch its tail.
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

  return (
    <div className="h-screen w-screen overflow-hidden flex flex-col bg-background text-foreground font-sans selection:bg-primary/30">
      {/* Reconnecting banner — shown only after a prior successful connection
          drops (mid-session Wi-Fi loss). A cold-load failure does not set
          reconnecting, so the banner stays hidden on first load. The pulsing
          dot uses the animate-pulse utility; semantic tokens only. */}
      {backend.reconnecting && (
        <Banner
          variant={backend.reconnectFailed ? 'error' : 'info'}
          role={backend.reconnectFailed ? 'alert' : 'status'}
          className="flex items-center gap-2 px-3 py-1.5 border-b shrink-0"
        >
          {backend.reconnectFailed ? (
            <>
              <WifiOff className="w-3.5 h-3.5 shrink-0" />
              <span>Connection lost — the backend has been unreachable for a while.</span>
              <button
                type="button"
                onClick={() => backend.reconnectNow()}
                className="shrink-0 underline hover:no-underline transition"
              >
                Retry now
              </button>
            </>
          ) : (
            <>
              <span className="inline-block h-2 w-2 rounded-full bg-muted-foreground/70 animate-pulse" />
              Reconnecting…
            </>
          )}
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
                onCloseOthers={handleCloseOthers}
                onCloseSaved={handleCloseSaved}
                onCloseToRight={handleCloseToRight}
                onCopyPath={handleCopyPath}
                onCopyRelativePath={handleCopyRelativePath}
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
            <button
              type="button"
              title="Command Palette (Ctrl+P)"
              aria-label="Command Palette"
              onClick={() => window.dispatchEvent(new CustomEvent('command-palette-open'))}
              className="shrink-0 self-center mr-2 w-7 h-6 rounded flex items-center justify-center bg-secondary text-secondary-foreground hover:bg-accent transition"
            >
              <Command className="w-3.5 h-3.5" />
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

      {/* Left Sidebar — workspace switcher + explorer, search, or source control.
       * Wrapped in a compact error boundary so a render crash here doesn't
       * unmount the editor or chat. */}
      <ErrorBoundary compact name="Sidebar">
      <LeftSidebar
        activePanel={leftPanel}
        onSwitchPanel={setLeftPanel}
        fileTree={fileTree}
        visible={showLeftSidebar}
        onFileSelect={handleFileSelect}
        onOpenPreview={handleOpenPreview}
        onCopyPath={handleCopyPath}
        onCopyRelativePath={handleCopyRelativePath}
        onRename={handleTreeRename}
        onDelete={handleTreeDelete}
        onNewFile={handleTreeNewFile}
        onNewFolder={handleTreeNewFolder}
        workspaces={backend.workspaces}
        activeWorkspace={backend.activeWorkspace}
        onWorkspaceSelect={backend.selectWorkspace}
        onSearchResultSelect={handleSearchResultSelect}
        onOpenDiff={handleOpenDiff}
        onOpenCommitDiff={handleOpenCommitDiff}
        onRepoChanged={refreshGitState}
        style={isDesktop ? { width: leftPanelWidth } : undefined}
        connected={backend.connected}
      />
      </ErrorBoundary>

      {/* Resize handle between left sidebar and editor (desktop only) */}
      {isDesktop && showLeftSidebar && (
        <div
          role="separator"
          tabIndex={0}
          aria-orientation="vertical"
          aria-valuenow={leftPanelWidth}
          aria-valuemin={LEFT_MIN}
          aria-valuemax={LEFT_MAX}
          onPointerDown={startLeftDrag}
          onKeyDown={(e) => {
            if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
            e.preventDefault()
            const step = e.key === 'ArrowRight' ? 16 : -16
            setLeftPanelWidth((w) => Math.min(LEFT_MAX, Math.max(LEFT_MIN, w + step)))
          }}
          className="w-1 cursor-col-resize bg-transparent hover:bg-accent-foreground/20 focus:outline-none focus-visible:bg-accent-foreground/30 transition-colors shrink-0 touch-none"
          title="Drag to resize"
        />
      )}

      {/* Compact error boundary isolates editor crashes from the rest of the app. */}
      <ErrorBoundary compact name="Editor">
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
        onCursorChange={setCursorPos}
        cursorPos={cursorPos}
        scrollToLine={searchResultLine}
        settingsProps={{
          agents: backend.agents,
          onAddAgent: backend.addAgent,
          onDeleteAgent: backend.deleteAgent,
          onAutodetect: backend.autodetectAgents,
          activeSessionId,
          activeSection: settingsSection,
          onSectionChange: setSettingsSection,
          workspaceId: backend.activeWorkspace?.id,
          workspaceTrusted: backend.activeWorkspace?.trusted,
          onSetWorkspaceTrust: backend.setWorkspaceTrust,
          editorSettings,
          onEditorSettingsChange: updateEditorSettings,
        }}
        hideTabBar={isDesktop}
        wrap={wrap}
        onToggleWrap={() => setWrap(!wrap)}
        onToggleViewMode={handleToggleViewMode}
        onOpenBrowsePreview={handleOpenPreview}
        events={backend.events as AppEvent[]}
        isDesktop={isDesktop}
        workspaceName={backend.activeWorkspace?.name}
        gitBranch={gitBranch}
        trusted={backend.activeWorkspace?.trusted}
        onCloseOthers={handleCloseOthers}
        onCloseSaved={handleCloseSaved}
        onCloseToRight={handleCloseToRight}
        onCopyPath={handleCopyPath}
        onCopyRelativePath={handleCopyRelativePath}
        onKeepOpen={handleKeepOpen}
        fontSize={fontSize}
        onFontSizeChange={setFontSize}
        tabSize={editorSettings.tabSize}
        lineNumbers={editorSettings.lineNumbers}
        foldGutter={editorSettings.foldGutter}
        bracketMatching={editorSettings.bracketMatching}
        autoIndent={editorSettings.autoIndent}
        closeBrackets={editorSettings.closeBrackets}
      />
      </ErrorBoundary>

      {/* Resize handle between editor and right chat panel (desktop only) */}
      {isDesktop && showChat && (
        <div
          role="separator"
          tabIndex={0}
          aria-orientation="vertical"
          aria-valuenow={rightPanelWidth}
          aria-valuemin={RIGHT_MIN}
          aria-valuemax={RIGHT_MAX}
          onPointerDown={startRightDrag}
          onKeyDown={(e) => {
            if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
            e.preventDefault()
            // Inverted vs. the left handle: ArrowLeft widens the right panel,
            // matching the mouse drag direction (drag left edge leftward).
            const step = e.key === 'ArrowLeft' ? 16 : -16
            setRightPanelWidth((w) => Math.min(RIGHT_MAX, Math.max(RIGHT_MIN, w + step)))
          }}
          className="w-1 cursor-col-resize bg-transparent hover:bg-accent-foreground/20 focus:outline-none focus-visible:bg-accent-foreground/30 transition-colors shrink-0 touch-none"
          title="Drag to resize"
        />
      )}

      {/* Right Sidebar — agent chat. Compact boundary keeps a chat crash from
       * taking down the editor/sidebar. */}
      <ErrorBoundary compact name="Chat">
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
        pendingCreatedSessionIds={backend.pendingCreatedSessionIds}
        pendingClosedSessionIds={backend.pendingClosedSessionIds}
        hasOlderEvents={backend.hasOlderSessionEvents}
        onConsumeSessionCreated={backend.consumeSessionCreated}
        onConsumeSessionClosed={backend.consumeSessionClosed}
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
          onLoadOlder: backend.loadOlderSessionEvents,
          onUploadFile: backend.uploadFile,
          onSelectWorkspace: (id) => {
            const workspace = backend.workspaces.find((candidate) => candidate.id === id)
            if (workspace) backend.selectWorkspace(workspace)
          },
        }}
        style={isDesktop ? { width: rightPanelWidth } : undefined}
        onOpenMcpSettings={() => {
          openSettingsTab('mcp-servers')
          if (!isDesktop) setMobileView('editor')
        }}
      />
      </ErrorBoundary>

      </div>

      {/* Status Bar (desktop only) — spans the full app width, sitting below the
       *  editor + chat shell and above the (desktop-hidden) mobile nav. On
       *  mobile the StatusBar is rendered inside EditorPane instead. */}
      {isDesktop && (
        <StatusBar
          activeTab={openTabs.find((t) => t.id === activeTabId) || null}
          fontSize={fontSize}
          onFontSizeChange={setFontSize}
          gitBranch={gitBranch}
          cursorPos={cursorPos}
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
      <CommandPalette
        fileTree={fileTree}
        onFileSelect={handleFileSelect}
        commands={commands}
      />
    </div>
  )
}
