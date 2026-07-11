import { useState, useEffect } from 'react'
import type { CSSProperties } from 'react'
import { FolderCode, ChevronsUpDown, Wifi, WifiOff, Files, Search, Check } from 'lucide-react'
import { cn } from '@/lib/utils'
import { FileTree } from './FileTree'
import { SearchPanel } from './SearchPanel'
import type { FileTreeNode, LeftPanel } from '@/types'

/**
 * Left sidebar — workspace switcher + file tree or search (Blueprint Sec 17).
 * On desktop: visible alongside editor. On mobile: full-screen overlay.
 */
export function LeftSidebar({
  activePanel,
  onSwitchPanel,
  fileTree,
  visible,
  onFileSelect,
  workspaces,
  activeWorkspace,
  onWorkspaceSelect,
  onSearchResultSelect,
  style,
  connected = true,
}: {
  activePanel: LeftPanel
  onSwitchPanel: (panel: LeftPanel) => void
  fileTree: FileTreeNode[]
  visible: boolean
  onFileSelect: (path: string) => void
  workspaces: { id: string; name: string; path: string }[]
  activeWorkspace: { id: string; name: string; path: string } | null
  onWorkspaceSelect: (ws: { id: string; name: string; path: string }) => void
  /** Called when a search result is clicked — opens the file in the editor. */
  onSearchResultSelect?: (path: string, lineNumber: number) => void
  /** Optional inline style — used by App.tsx to apply a persisted panel width on desktop. */
  style?: CSSProperties
  /** Whether the backend WebSocket is connected. Drives the Online/Offline badge. */
  connected?: boolean
}) {
  const [showWorkspaceDropdown, setShowWorkspaceDropdown] = useState(false)

  // Close the workspace dropdown on Escape (keyboard accessibility).
  useEffect(() => {
    if (!showWorkspaceDropdown) return
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        setShowWorkspaceDropdown(false)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [showWorkspaceDropdown])

  /** Mini horizontal activity bar for mobile (files/search toggle). */
  const miniTabs: { id: LeftPanel; icon: typeof Files; label: string }[] = [
    { id: 'files', icon: Files, label: 'Files' },
    { id: 'search', icon: Search, label: 'Search' },
  ]

  return (
    <aside
      className={cn(
        'flex-col h-full shrink-0 w-full bg-panel border-r border-border lg:w-60',
        visible ? 'flex' : 'hidden',
        'absolute inset-0 z-30 lg:relative lg:inset-auto lg:z-auto',
      )}
      style={style}
    >
      {/* Workspace Switcher (mobile only, visible at the very top) */}
      <div className="lg:hidden p-3 border-b border-border shrink-0 flex flex-col gap-3">
        {/* Online Indicator (Top Left) */}
        <div className="flex items-start">
          <div
            className={cn(
              'flex items-center gap-1.5 text-xs px-2 py-0.5 rounded-full border',
              connected
                ? 'text-green-400 bg-green-400/10 border-green-500/20'
                : 'text-muted-foreground bg-muted/40 border-border',
            )}
            title={connected ? 'Connected to backend' : 'Backend offline — reconnecting…'}
          >
            {connected ? (
              <>
                <Wifi className="w-4 h-4" /> Online
              </>
            ) : (
              <>
                <WifiOff className="w-4 h-4 animate-pulse" /> Offline
              </>
            )}
          </div>
        </div>

        {/* Workspace Selector (Below Online Indicator) */}
        <div className="relative">
          <button
            onClick={() => setShowWorkspaceDropdown(!showWorkspaceDropdown)}
            className="w-full bg-background border border-input rounded-md px-3 py-2 flex items-center justify-between hover:border-muted-foreground transition shadow-sm"
            aria-label="Switch workspace"
            aria-expanded={showWorkspaceDropdown}
            aria-haspopup="listbox"
          >
            <div className="flex items-center gap-2 min-w-0">
              <FolderCode className="w-4 h-4 text-primary shrink-0" />
              <span className="truncate text-xs font-medium">{activeWorkspace?.name || 'No workspace'}</span>
            </div>
            <ChevronsUpDown className="w-4 h-4 text-muted-foreground shrink-0" />
          </button>
          
          {/* Dropdown Menu */}
          {showWorkspaceDropdown && (
            <>
              <div className="fixed inset-0 z-40" onClick={() => setShowWorkspaceDropdown(false)} aria-hidden="true" />
              <div className="absolute top-full left-0 right-0 mt-1 bg-panel border border-border rounded-lg shadow-xl z-50 py-1 max-h-60 overflow-y-auto" role="listbox" aria-label="Workspaces">
                {workspaces.map((ws) => (
                  <button
                    key={ws.id}
                    onClick={() => {
                      onWorkspaceSelect(ws)
                      setShowWorkspaceDropdown(false)
                    }}
                    className="w-full text-left px-3 py-2 text-xs hover:bg-accent flex items-center justify-between transition group"
                  >
                    <span className="truncate">{ws.name}</span>
                    {activeWorkspace?.id === ws.id && (
                      <Check className="w-3.5 h-3.5 text-primary" />
                    )}
                  </button>
                ))}
                {workspaces.length === 0 && (
                  <div className="px-3 py-2 text-xs text-muted-foreground italic">No workspaces found</div>
                )}
              </div>
            </>
          )}
        </div>
      </div>

      {/* Mini horizontal activity bar (mobile only) */}
      <div className="flex lg:hidden items-center gap-1 px-3 py-1.5 border-b border-border shrink-0 bg-activity-bar">
        {miniTabs.map(({ id, icon: Icon, label }) => (
          <button
            key={id}
            onClick={() => onSwitchPanel(id)}
            className={cn(
              'flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs font-medium transition',
              activePanel === id
                ? 'text-primary bg-primary/10'
                : 'text-muted-foreground hover:text-foreground',
            )}
          >
            <Icon className="w-4 h-4" /> {label}
          </button>
        ))}
      </div>

      {/* Files panel */}
      {activePanel === 'files' && (
        <>
          {/* File Tree */}
          <div className="px-3 py-2 text-[10px] font-semibold text-muted-foreground uppercase tracking-wider shrink-0">
            Explorer
          </div>
          <FileTree nodes={fileTree} onFileSelect={onFileSelect} workspaceId={activeWorkspace?.id ?? null} />
        </>
      )}

      {/* Search panel */}
      {activePanel === 'search' && (
        <SearchPanel
          workspaceId={activeWorkspace?.id ?? null}
          onSelectResult={onSearchResultSelect}
        />
      )}
    </aside>
  )
}
