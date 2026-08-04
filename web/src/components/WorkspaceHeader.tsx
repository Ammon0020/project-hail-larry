import { useState, useEffect } from 'react'
import { FolderCode, ChevronsUpDown, Wifi, WifiOff, Check } from 'lucide-react'
import { cn } from '@/lib/utils'

export function WorkspaceHeader({
  connected,
  workspaces,
  activeWorkspace,
  onWorkspaceSelect,
}: {
  connected: boolean
  workspaces: { id: string; name: string; path: string }[]
  activeWorkspace: { id: string; name: string; path: string } | null
  onWorkspaceSelect: (ws: { id: string; name: string; path: string }) => void
}) {
  const [showWorkspaceDropdown, setShowWorkspaceDropdown] = useState(false)
  const [showStatusDropdown, setShowStatusDropdown] = useState(false)

  // Close dropdowns on Escape
  useEffect(() => {
    if (!showWorkspaceDropdown && !showStatusDropdown) return
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        setShowWorkspaceDropdown(false)
        setShowStatusDropdown(false)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [showWorkspaceDropdown, showStatusDropdown])

  return (
    <div className="flex items-center gap-2 w-full min-w-0 @container">
      {/* Online indicator */}
      <div className="relative shrink-0">
        <button
          onClick={() => {
            setShowStatusDropdown(!showStatusDropdown)
            setShowWorkspaceDropdown(false)
          }}
          className={cn(
            'flex items-center gap-1.5 text-xs px-2 py-0.5 rounded-full border transition hover:opacity-80 cursor-pointer',
            connected
              ? 'text-green-400 bg-green-400/10 border-green-500/20'
              : 'text-muted-foreground bg-muted/40 border-border'
          )}
          title={connected ? 'Connected to backend' : 'Backend offline — reconnecting…'}
        >
          {connected ? (
            <>
              <Wifi className="w-4 h-4 shrink-0" />
              <span className="hidden @[220px]:inline">Online</span>
            </>
          ) : (
            <>
              <WifiOff className="w-4 h-4 animate-pulse shrink-0" />
              <span className="hidden @[220px]:inline animate-pulse">Offline</span>
            </>
          )}
        </button>

        {showStatusDropdown && (
          <>
            <div className="fixed inset-0 z-40" onClick={() => setShowStatusDropdown(false)} aria-hidden="true" />
            <div className="absolute top-full left-0 mt-2 w-56 bg-panel border border-border rounded-md shadow-xl z-50 p-3 text-xs text-muted-foreground cursor-default leading-relaxed">
              <div className="font-semibold text-foreground mb-1.5 flex items-center gap-1.5">
                 {connected ? <Wifi className="w-4 h-4 text-green-400"/> : <WifiOff className="w-4 h-4 text-muted-foreground"/>}
                 Connection Status
              </div>
              {connected ? (
                <p>The workspace is online and connected via local WebSocket. Your files and code edits are synced automatically.</p>
              ) : (
                <p>The backend daemon is disconnected. The app will reconnect automatically when it becomes available in the background.</p>
              )}
            </div>
          </>
        )}
      </div>

      <div className="flex-1 min-w-0" />

      {/* Workspace Label */}
      <span className="text-[10px] text-muted-foreground uppercase font-bold tracking-wider shrink-0 hidden @[340px]:inline">Workspace:</span>

      {/* Workspace Selector */}
      <div className="relative shrink-0 w-[140px] @[280px]:w-[160px]">
        <button
          onClick={() => {
            setShowWorkspaceDropdown(!showWorkspaceDropdown)
            setShowStatusDropdown(false)
          }}
          className="w-full bg-background border border-input rounded-md px-2 py-1 flex items-center justify-between hover:border-muted-foreground transition shadow-sm cursor-pointer"
          aria-label="Switch workspace"
          aria-expanded={showWorkspaceDropdown}
          aria-haspopup="listbox"
        >
          <div className="flex items-center gap-1.5 min-w-0">
            <FolderCode className="w-4 h-4 text-primary shrink-0" />
            <span className="truncate text-xs font-medium">{activeWorkspace?.name || 'No workspace'}</span>
          </div>
          <ChevronsUpDown className="w-4 h-4 text-muted-foreground shrink-0" />
        </button>
        {/* Dropdown Menu */}
        {showWorkspaceDropdown && (
          <>
            <div className="fixed inset-0 z-40" onClick={() => setShowWorkspaceDropdown(false)} aria-hidden="true" />
            {/* divide-y and divide-border for visual separators, right-0 aligns to right side of parent on mobile */}
            <div className="absolute top-full right-0 lg:left-0 lg:right-auto mt-2 w-[calc(100vw-32px)] lg:w-56 max-w-[280px] bg-panel border border-border rounded-md shadow-xl z-50 max-h-60 overflow-y-auto divide-y divide-border" role="listbox" aria-label="Workspaces">
              {workspaces.map((ws) => (
                <button
                  key={ws.id}
                  onClick={() => {
                    onWorkspaceSelect(ws)
                    setShowWorkspaceDropdown(false)
                  }}
                  className="w-full text-left px-3 py-2.5 text-xs hover:bg-accent flex items-center justify-between transition group"
                >
                  <span className="truncate">{ws.name}</span>
                  {activeWorkspace?.id === ws.id && (
                    <Check className="w-3.5 h-3.5 text-primary" />
                  )}
                </button>
              ))}
              {workspaces.length === 0 && (
                <div className="px-3 py-3 text-xs text-muted-foreground italic text-center">No workspaces found</div>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  )
}
