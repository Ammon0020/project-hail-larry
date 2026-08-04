import { useEffect, useState } from 'react'
import { type SettingsSection } from '@/components/SettingsPanel'
import { usePanelResize } from '@/hooks/usePanelResize'
import type { LeftPanel, MobileView } from '@/types'

type ReadValidString = <T extends string>(
  key: string,
  allowed: readonly T[],
  fallback: T,
) => T

export function useLayoutState(readValidString: ReadValidString) {
  // Panel state — restored from localStorage so the layout survives a reload.
  // Validated against the allowed union values so a corrupted/old entry cannot
  // produce an invalid panel id (AGENTS.md — type safety).
  const [leftPanel, setLeftPanel] = useState<LeftPanel>(
    () => readValidString('lai:leftPanel', ['files', 'search', 'git'] as const, 'files'),
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

  // Settings panel subsection (e.g. harnesses, mcp-servers, theme) — owned
  // here so deep-links (MCP popout Settings icon) can focus a subsection
  // without an event bus.
  const [settingsSection, setSettingsSection] = useState<SettingsSection>('harnesses')

  /** Track viewport changes for responsive layout switching. */
  useEffect(() => {
    const handleResize = () => setIsDesktop(window.innerWidth >= 1024)
    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [])

  useEffect(() => {
    localStorage.setItem('lai:leftPanel', leftPanel)
  }, [leftPanel])

  useEffect(() => {
    localStorage.setItem('lai:mobileView', mobileView)
  }, [mobileView])

  // ---- Determine panel visibility based on viewport and state ----
  // On desktop, the sidebar can be hidden via Ctrl+B (width set to 0).
  const sidebarHidden = isDesktop && leftPanelWidth === 0
  const showLeftSidebar = (isDesktop && !sidebarHidden) || mobileView === 'explorer'
  const showEditor = isDesktop || mobileView === 'editor'
  const showChat = (isDesktop && rightPanelWidth > 0) || mobileView === 'chat'

  return {
    isDesktop,
    setIsDesktop,
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
    sidebarHidden,
    showLeftSidebar,
    showEditor,
    showChat,
  }
}
