import { useState, useEffect } from 'react'
import { CloudUpload, CloudOff, FolderCode, ChevronsUpDown, Wifi, WifiOff, Check } from 'lucide-react'
import { cn } from '@/lib/utils'

export function WorkspaceHeader({
  connected,
  workspaces,
  activeWorkspace,
  onWorkspaceSelect,
  syncTabs,
  onSyncTabsToggle,
}: {
  connected: boolean
  workspaces: { id: string; name: string; path: string }[]
  activeWorkspace: { id: string; name: string; path: string } | null
  onWorkspaceSelect: (ws: { id: string; name: string; path: string }) => void
  /** `true`/`undefined` = sync (default); `false` = browser-local. */
  syncTabs?: boolean | null
  /** Flips the active workspace's syncing preference via the API. */
  onSyncTabsToggle?: (next: boolean) => void
}) {
  const [showWorkspaceDropdown, setShowWorkspaceDropdown] = useState(false)
  const [showStatusDropdown, setShowStatusDropdown] = useState(false)
  const syncOn = syncTabs ?? true

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
    <div className="@container flex w-full min-w-0 items-center gap-2">
      {/* Online indicator */}
      <div className="relative shrink-0">
        <button
          onClick={() => {
            setShowStatusDropdown(!showStatusDropdown)
            setShowWorkspaceDropdown(false)
          }}
          className={cn(
            'flex cursor-pointer items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs transition hover:opacity-80',
            connected
              ? 'border-green-500/20 bg-green-400/10 text-green-400'
              : 'border-border bg-muted/40 text-muted-foreground'
          )}
          title={connected ? 'Connected to backend' : 'Backend offline — reconnecting…'}
        >
          {connected ? (
            <Wifi className="size-4 shrink-0" />
          ) : (
            <WifiOff className="size-4 shrink-0 animate-pulse" />
          )}
        </button>

        {showStatusDropdown && (
          <>
            <div className="fixed inset-0 z-40" onClick={() => setShowStatusDropdown(false)} aria-hidden="true" />
            <div className="absolute top-full left-0 z-50 mt-2 w-56 cursor-default rounded-md border border-border bg-panel p-3 text-xs leading-relaxed text-muted-foreground shadow-xl">
              <div className="mb-1.5 flex items-center gap-1.5 font-semibold text-foreground">
                 {connected ? <Wifi className="size-4 text-green-400"/> : <WifiOff className="size-4 text-muted-foreground"/>}
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

      {/* Tab-syncing toggle — on = server-shared, off = browser-local. */}
      {activeWorkspace && onSyncTabsToggle && (
        <button
          type="button"
          onClick={() => onSyncTabsToggle(!syncOn)}
          aria-label={`Workspace tab syncing ${syncOn ? 'on' : 'off'}`}
          aria-pressed={syncOn}
          title={`Workspace tab syncing ${syncOn ? 'on' : 'off'}`}
          className={cn(
            'flex size-6 shrink-0 cursor-pointer items-center justify-center rounded-full border transition hover:opacity-80',
            syncOn
              ? 'border-primary/20 bg-primary/10 text-primary'
              : 'border-border bg-muted/40 text-muted-foreground',
          )}
        >
          {syncOn ? (
            <CloudUpload className="size-3.5" />
          ) : (
            <CloudOff className="size-3.5" />
          )}
        </button>
      )}

      <div className="min-w-0 flex-1" />

      {/* Workspace Label */}
      <span className="hidden shrink-0 text-[10px] font-bold tracking-wider text-muted-foreground uppercase @[340px]:inline">Workspace:</span>

      {/* Workspace Selector */}
      <div className="relative w-35 shrink-0 @[280px]:w-40">
        <button
          onClick={() => {
            setShowWorkspaceDropdown(!showWorkspaceDropdown)
            setShowStatusDropdown(false)
          }}
          className="flex w-full cursor-pointer items-center justify-between rounded-md border border-input bg-background px-2 py-1 shadow-sm transition hover:border-muted-foreground"
          aria-label="Switch workspace"
          aria-expanded={showWorkspaceDropdown}
          aria-haspopup="listbox"
        >
          <div className="flex min-w-0 items-center gap-1.5">
            <FolderCode className="size-4 shrink-0 text-primary" />
            <span className="truncate text-xs font-medium">{activeWorkspace?.name || 'No workspace'}</span>
          </div>
          <ChevronsUpDown className="size-4 shrink-0 text-muted-foreground" />
        </button>
        {/* Dropdown Menu */}
        {showWorkspaceDropdown && (
          <>
            <div className="fixed inset-0 z-40" onClick={() => setShowWorkspaceDropdown(false)} aria-hidden="true" />
            {/* divide-y and divide-border for visual separators, right-0 aligns to right side of parent on mobile */}
            <div className="absolute top-full right-0 z-50 mt-2 max-h-60 w-[calc(100vw-32px)] max-w-70 divide-y divide-border overflow-y-auto rounded-md border border-border bg-panel shadow-xl lg:right-auto lg:left-0 lg:w-56" role="listbox" aria-label="Workspaces">
              {workspaces.map((ws) => (
                <button
                  key={ws.id}
                  onClick={() => {
                    onWorkspaceSelect(ws)
                    setShowWorkspaceDropdown(false)
                  }}
                  className="group flex w-full items-center justify-between px-3 py-2.5 text-left text-xs transition hover:bg-accent"
                >
                  <span className="truncate">{ws.name}</span>
                  {activeWorkspace?.id === ws.id && (
                    <Check className="size-3.5 text-primary" />
                  )}
                </button>
              ))}
              {workspaces.length === 0 && (
                <div className="p-3 text-center text-xs text-muted-foreground italic">No workspaces found</div>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  )
}
