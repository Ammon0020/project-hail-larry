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

  // Panel state
  const [leftPanel, setLeftPanel] = useState<LeftPanel>('files')
  const [mobileView, setMobileView] = useState<MobileView>('editor')
  const [isDesktop, setIsDesktop] = useState(
    typeof window !== 'undefined' ? window.innerWidth >= 1024 : true,
  )
  const [isSettingsModalOpen, setIsSettingsModalOpen] = useState(false)

  // Tab state
  const [openTabs, setOpenTabs] = useState<Tab[]>([])
  const [activeTabId, setActiveTabId] = useState<string | null>(null)

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
  const showLeftSidebar = isDesktop || mobileView === 'explorer'
  const showEditor = isDesktop || mobileView === 'editor'
  const showChat = isDesktop || mobileView === 'chat'
  const showMobileSettings = !isDesktop && mobileView === 'settings'

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
      />

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
