import type { CSSProperties } from 'react'
import { Files, Search } from 'lucide-react'
import { cn } from '@/lib/utils'
import { FileTree } from './FileTree'
import { SearchPanel } from './SearchPanel'
import type { FileTreeNode, LeftPanel } from '@/types'
import { WorkspaceHeader } from './WorkspaceHeader'

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
      <div className="lg:hidden p-2 border-b border-border shrink-0">
        <WorkspaceHeader 
          connected={connected}
          workspaces={workspaces}
          activeWorkspace={activeWorkspace}
          onWorkspaceSelect={onWorkspaceSelect}
        />
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
