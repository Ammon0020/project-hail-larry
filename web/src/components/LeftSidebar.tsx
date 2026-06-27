import { useState, useEffect } from 'react'
import type { CSSProperties } from 'react'
import { FolderCode, ChevronsUpDown, Wifi, Files, Search, Check } from 'lucide-react'
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
        'flex-col h-full shrink-0 w-full bg-panel border-r border-gray-800 lg:w-60',
        visible ? 'flex' : 'hidden',
        'absolute inset-0 z-30 lg:relative lg:inset-auto lg:z-auto',
      )}
      style={style}
    >
      {/* Mini horizontal activity bar (mobile only) */}
      <div className="flex lg:hidden items-center gap-1 px-3 py-1.5 border-b border-gray-800/50 shrink-0 bg-activity-bar">
        {miniTabs.map(({ id, icon: Icon, label }) => (
          <button
            key={id}
            onClick={() => onSwitchPanel(id)}
            className={cn(
              'flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs font-medium transition',
              activePanel === id
                ? 'text-blue-400 bg-blue-600/10'
                : 'text-gray-500 hover:text-gray-300',
            )}
          >
            <Icon className="w-4 h-4" /> {label}
          </button>
        ))}
      </div>

      {/* Files panel */}
      {activePanel === 'files' && (
        <>
          {/* Workspace Switcher (Blueprint Sec 13) */}
          <div className="p-3 border-b border-gray-800 shrink-0">
            <div className="flex items-center justify-between mb-2">
              <label className="text-[10px] text-gray-500 uppercase font-bold tracking-wider">Workspace</label>
              <div className="flex items-center gap-1 text-[10px] text-green-400 bg-green-400/10 px-2 py-0.5 rounded-full border border-green-500/20">
                <Wifi className="w-3 h-3" /> Online
              </div>
            </div>
            <div className="relative">
              <button
                onClick={() => setShowWorkspaceDropdown(!showWorkspaceDropdown)}
                className="w-full bg-background border border-gray-700 rounded-lg p-2 flex items-center justify-between hover:border-gray-500 transition shadow-sm"
                aria-label="Switch workspace"
                aria-expanded={showWorkspaceDropdown}
                aria-haspopup="listbox"
              >
                <div className="flex items-center gap-2 overflow-hidden">
                  <FolderCode className="w-4 h-4 text-blue-400 shrink-0" />
                  <span className="truncate text-xs font-medium">{activeWorkspace?.name || 'No workspace'}</span>
                </div>
                <ChevronsUpDown className="w-3.5 h-3.5 text-gray-500 shrink-0" />
              </button>
              
              {/* Dropdown Menu */}
              {showWorkspaceDropdown && (
                <>
                  <div className="fixed inset-0 z-40" onClick={() => setShowWorkspaceDropdown(false)} aria-hidden="true" />
                  <div className="absolute top-full left-0 right-0 mt-1 bg-panel border border-gray-700 rounded-lg shadow-xl z-50 py-1 max-h-60 overflow-y-auto" role="listbox" aria-label="Workspaces">
                    {workspaces.map((ws) => (
                      <button
                        key={ws.id}
                        onClick={() => {
                          onWorkspaceSelect(ws)
                          setShowWorkspaceDropdown(false)
                        }}
                        className="w-full text-left px-3 py-2 text-xs hover:bg-blue-500/20 flex items-center justify-between transition group"
                      >
                        <span className="truncate">{ws.name}</span>
                        {activeWorkspace?.id === ws.id && (
                          <Check className="w-3.5 h-3.5 text-blue-400" />
                        )}
                      </button>
                    ))}
                    {workspaces.length === 0 && (
                      <div className="px-3 py-2 text-xs text-gray-500 italic">No workspaces found</div>
                    )}
                  </div>
                </>
              )}
            </div>
          </div>

          {/* File Tree */}
          <div className="px-3 py-2 text-[10px] font-semibold text-gray-500 uppercase tracking-wider shrink-0">
            Explorer
          </div>
          <FileTree nodes={fileTree} onFileSelect={onFileSelect} />
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
