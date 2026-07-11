import { useEffect, useRef } from 'react'
import { Loader2, Settings, ShoppingBag } from 'lucide-react'
import { cn } from '../../lib/utils'

interface McpPopoutProps {
  /** MCP server list — same shape ChatPanel already owns. */
  mcpServers: { name: string; enabled: boolean }[]
  /** Toggle a server's enabled flag. */
  onToggle: (name: string, enabled: boolean) => void
  /** Server name currently being toggled (shows spinner on its row). */
  togglingServer: string | null
  /** Closes the popout (outside-click + escape). */
  onClose: () => void
}

/**
 * MCP servers popout — anchored above the Tools button in the chat composer.
 * Shows each configured MCP server with a status dot and a toggle switch.
 * Outside-click and Escape close the popout.
 *
 * Status dots: enabled → green (healthy), disabled → gray. The backend does
 * not expose a health field in v1, so unhealthy (red) is not populated yet.
 * See agent_chat_update.htm (MCP popout section).
 */
export function McpPopout({
  mcpServers,
  onToggle,
  togglingServer,
  onClose,
}: McpPopoutProps) {
  const ref = useRef<HTMLDivElement>(null)

  // Close on outside-click (mousedown so it fires before focus shifts) and
  // on Escape. Re-subscribes if onClose changes identity.
  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
    }
    function handleEscape(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('mousedown', handleClickOutside)
    document.addEventListener('keydown', handleEscape)
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
      document.removeEventListener('keydown', handleEscape)
    }
  }, [onClose])

  const enabledCount = mcpServers.filter((s) => s.enabled).length

  return (
    <div
      ref={ref}
      className="absolute bottom-full left-0 mb-2 w-[260px] z-50 bg-popover border border-border rounded-[10px] shadow-lg p-3 flex flex-col gap-3"
    >
      {/* Header: enabled count + store/settings actions. */}
      <div className="flex items-center justify-between pb-2 border-b border-white/[0.05]">
        <span className="text-xs font-medium text-muted-foreground">
          {enabledCount} MCP{enabledCount === 1 ? '' : 's'}
        </span>
        <div className="flex gap-1">
          {/* v1: store/settings buttons are no-ops — they just close the
              popout. No backend wiring yet. */}
          <button
            className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-white/[0.05] transition"
            title="MCP store"
            aria-label="MCP store"
            onClick={onClose}
          >
            <ShoppingBag className="w-3.5 h-3.5" />
          </button>
          <button
            className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-white/[0.05] transition"
            title="MCP settings"
            aria-label="MCP settings"
            onClick={onClose}
          >
            <Settings className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Server list. */}
      <div className="flex flex-col gap-2.5">
        {mcpServers.length === 0 && (
          <div className="text-xs text-muted-foreground py-2 text-center">
            No MCP servers configured
          </div>
        )}
        {mcpServers.map((server) => (
          <div
            key={server.name}
            className={cn(
              'flex items-center justify-between text-[13px] transition-colors',
              server.enabled ? 'text-foreground' : 'text-muted-foreground',
            )}
          >
            <div className="flex items-center gap-2.5">
              {/* v1: enabled=green, disabled=gray. No health field yet —
                  red/unhealthy not populated. */}
              <span
                className={cn(
                  'w-2 h-2 rounded-full',
                  server.enabled
                    ? 'bg-green-500 shadow-[0_0_8px_rgba(16,185,129,0.4)]'
                    : 'bg-muted-foreground',
                )}
              />
              <span>{server.name}</span>
            </div>

            {/* Custom toggle: hidden checkbox + styled spans (no Radix
                Switch dependency). While toggling, show a spinner in place
                of the track + knob. */}
            <label className="relative inline-block w-8 h-[18px] shrink-0">
              <input
                type="checkbox"
                checked={server.enabled}
                disabled={togglingServer === server.name}
                onChange={(e) => onToggle(server.name, e.target.checked)}
                className="opacity-0 w-0 h-0"
              />
              {togglingServer === server.name ? (
                <Loader2 className="w-3 h-3 animate-spin absolute top-[2px] left-[9px] text-muted-foreground" />
              ) : (
                <>
                  <span
                    className={cn(
                      'absolute inset-0 rounded-full border transition-colors',
                      server.enabled
                        ? 'bg-primary border-primary'
                        : 'bg-input border-border',
                    )}
                  />
                  <span
                    className={cn(
                      'absolute top-[2px] left-[2px] w-3 h-3 rounded-full transition-transform',
                      server.enabled
                        ? 'translate-x-[14px] bg-white'
                        : 'bg-muted-foreground',
                    )}
                  />
                </>
              )}
            </label>
          </div>
        ))}
      </div>
    </div>
  )
}
