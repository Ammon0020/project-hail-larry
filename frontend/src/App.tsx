import { useState, useEffect } from 'react'
import { LockScreen } from '@/components/LockScreen'
import { ActivityBar } from '@/components/ActivityBar'
import { LeftSidebar } from '@/components/LeftSidebar'
import { EditorPane } from '@/components/EditorPane'
import { ChatPanel } from '@/components/ChatPanel'
import { MobileNav } from '@/components/MobileNav'
import { MobileSettings } from '@/components/MobileSettings'
import { useMockBackend } from '@/hooks/useMockBackend'
import {
  mockFileTree,
  mockAgents,
  mockSessions,
  mockDevices,
  mockEvents,
  mockCodeContent,
} from '@/data/mockData'
import type { LeftPanel, MobileView } from '@/types'

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
  // Pairing state (Blueprint Sec 19)
  const [paired, setPaired] = useState(false)

  // Panel state
  const [leftPanel, setLeftPanel] = useState<LeftPanel>('files')
  const [mobileView, setMobileView] = useState<MobileView>('editor')
  const [isDesktop, setIsDesktop] = useState(
    typeof window !== 'undefined' ? window.innerWidth >= 1024 : true,
  )

  // Devices (Blueprint Sec 19 — revocable credentials)
  const [devices, setDevices] = useState(mockDevices)

  // Mock backend (replaced by real WebSocket in production)
  const { events, sendPrompt, respondPermission } = useMockBackend(mockEvents)

  /** Track viewport changes for responsive layout switching. */
  useEffect(() => {
    const handleResize = () => setIsDesktop(window.innerWidth >= 1024)
    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [])

  /** Revokes a paired device's access (Blueprint Sec 19). */
  const revokeDevice = (id: string) => {
    setDevices((prev) => prev.filter((d) => d.id !== id))
  }

  // ---- Lock screen for unpaired devices ----
  if (!paired) {
    return <LockScreen onPaired={() => setPaired(true)} />
  }

  // ---- Determine panel visibility based on viewport and state ----
  // Desktop: left sidebar + editor + chat always visible
  // Mobile: one panel at a time based on mobileView
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
        fileTree={mockFileTree}
        visible={showLeftSidebar}
      />

      {/* Center Editor — tabbed CodeMirror 6 with status bar */}
      <EditorPane content={mockCodeContent} visible={showEditor} />

      {/* Right Sidebar — agent chat */}
      <ChatPanel
        events={events}
        agents={mockAgents}
        sessions={mockSessions}
        visible={showChat}
        onSendMessage={(content) => sendPrompt('s1', content)}
        onPermissionResponse={respondPermission}
      />

      {/* Mobile Settings Panel (full-screen overlay) */}
      <MobileSettings
        devices={devices}
        visible={showMobileSettings}
        onRevokeDevice={revokeDevice}
      />

      {/* Mobile Bottom Nav (hidden on desktop) */}
      <MobileNav activeView={mobileView} onSwitchView={setMobileView} />
    </div>
  )
}
