import { useState, useEffect } from 'react'
import { LockScreen } from '@/components/LockScreen'
import { ActivityBar } from '@/components/ActivityBar'
import { LeftSidebar } from '@/components/LeftSidebar'
import { EditorPane } from '@/components/EditorPane'
import { ChatPanel } from '@/components/ChatPanel'
import { MobileNav } from '@/components/MobileNav'
import { MobileSettings } from '@/components/MobileSettings'
import { useBackend } from '@/hooks/useBackend'
import { mockAgents, mockSessions } from '@/data/mockData'
import type { LeftPanel, MobileView, FileTreeNode, AppEvent, Session } from '@/types'

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

  // Real backend connection
  const backend = useBackend()

  /** Track viewport changes for responsive layout switching. */
  useEffect(() => {
    const handleResize = () => setIsDesktop(window.innerWidth >= 1024)
    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [])

  // ---- Lock screen for unpaired devices ----
  if (!paired) {
    return <LockScreen onPaired={() => setPaired(true)} />
  }

  // Convert backend file tree to the component's expected format.
  const fileTree: FileTreeNode[] = backend.fileTree.map((n) => ({
    name: n.name,
    type: n.type as 'folder' | 'file',
    children: n.children?.map((c) => ({
      name: c.name,
      type: c.type as 'folder' | 'file',
      children: c.children,
    })),
  }))

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
        onOpenSettings={() => !isDesktop && setMobileView('settings')}
      />

      {/* Left Sidebar — workspace switcher + file tree / search */}
      <LeftSidebar
        activePanel={leftPanel}
        onSwitchPanel={setLeftPanel}
        fileTree={fileTree}
        visible={showLeftSidebar}
      />

      {/* Center Editor — tabbed CodeMirror 6 with status bar */}
      <EditorPane content={''} visible={showEditor} />

      {/* Right Sidebar — agent chat */}
      <ChatPanel
        events={backend.events as AppEvent[]}
        agents={backend.agents.length > 0 ? backend.agents : mockAgents}
        sessions={
          backend.sessions.length > 0
            ? backend.sessions.map((s) => ({
                id: s.id,
                name: s.name,
                time: '',
                status: s.status as Session['status'],
              }))
            : mockSessions
        }
        visible={showChat}
        onSendMessage={(content) => {
          // Use the first session or create one implicitly.
          const sessionId = backend.sessions[0]?.id || 's1'
          backend.sendPrompt(sessionId, content)
        }}
        onPermissionResponse={(id, decision) =>
          backend.respondPermission(id, decision)
        }
      />

      {/* Mobile Settings Panel (full-screen overlay) */}
      <MobileSettings
        devices={backend.devices.map((d) => ({
          id: d.id,
          name: d.name,
          icon: 'monitor',
          pairedAt: d.pairedAt,
        }))}
        visible={showMobileSettings}
        onRevokeDevice={(id) => backend.revokeDevice(id)}
      />

      {/* Mobile Bottom Nav (hidden on desktop) */}
      <MobileNav activeView={mobileView} onSwitchView={setMobileView} />
    </div>
  )
}
