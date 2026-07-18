import { useEffect, useRef } from 'react'
import { Loader2, Settings, ShoppingBag } from 'lucide-react'
import { cn } from '../../lib/utils'

/**
 * Health status for a single MCP server, mirroring the backend's
 * `internal/mcp/health.go` CheckHealth result. See `McpServerStatus` in
 * `@/lib/api` for the wire shape.
 */
type McpHealthStatus = 'healthy' | 'unhealthy' | 'disabled' | 'unknown'

interface McpPopoutProps {
  /** MCP server list — same shape ChatPanel already owns. */
  mcpServers: { name: string; enabled: boolean }[]
  /**
   * Optional health status keyed by server name, fetched on demand from
   * `GET /api/mcp/status`. While absent (initial load / fetch failed), dots
   * fall back to the legacy enabled-based coloring so there's no flash.
   */
  statusByName?: Record<string, { status: McpHealthStatus; error?: string }>
  /** True while a health refresh is in flight — shows a spinner in the header. */
  statusLoading?: boolean
  /** Toggle a server's enabled flag. */
  onToggle: (name: string, enabled: boolean) => void
  /** Server name currently being toggled (shows spinner on its row). */
  togglingServer: string | null
  /** Closes the popout (outside-click + escape). */
  onClose: () => void
  /**
   * Opens app Settings focused on the MCP Servers section. Called before
   * `onClose` when the Settings header button is clicked.
   */
  onOpenMcpSettings?: () => void
}

/**
 * MCP servers popout — anchored above the Tools button in the chat composer.
 * Shows each configured MCP server with a status dot and a toggle switch.
 * Outside-click and Escape close the popout.
 *
 * Status dots reflect the backend health check (`internal/mcp/health.go`):
 *   healthy   → green (with glow)
 *   unhealthy → red/destructive (with glow); `error` surfaced via native
 *               `title` tooltip on the row
 *   disabled  → gray
 *   unknown   → muted (or falls back to enabled-based color while status
 *               has not been fetched yet)
 *
 * While `statusLoading` is true, a small spinner is shown in the header so
 * the user can tell a refresh is in flight even if dots haven't updated.
 */
export function McpPopout({
  mcpServers,
  statusByName,
  statusLoading,
  onToggle,
  togglingServer,
  onClose,
  onOpenMcpSettings,
}: McpPopoutProps) {
  const ref = useRef<HTMLDivElement>(null)

  // Close on outside-click (mousedown so it fires before focus shifts) and
  // on Escape. Re-subscribes if onClose changes identity.
  //
  // The Tools toggle button in ChatComposer is marked with `data-mcp-toggle`
  // so we can ignore mousedowns on it here — its own onClick handles the
  // toggle. Without this, a click on Tools while open would fire this
  // outside-click handler (closing the popout) and then the click handler
  // would reopen it, making the button unable to close the popout (#1).
  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      const target = e.target as Element | null
      if (!target || !ref.current) return
      if (ref.current.contains(target)) return
      // Ignore clicks on the Tools toggle itself — it owns its own toggle
      // semantics and would otherwise cause a close-then-reopen flicker.
      if (typeof target.closest === 'function' && target.closest('[data-mcp-toggle]')) {
        return
      }
      onClose()
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
      {/* Header: enabled count + store/settings actions. While a health
          refresh is in flight, a small spinner sits next to the count so
          the user can tell dots may update shortly. */}
      <div className="flex items-center justify-between pb-2 border-b border-white/[0.05]">
        <span className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
          {enabledCount} MCP{enabledCount === 1 ? '' : 's'}
          {statusLoading && (
            <Loader2 className="w-3 h-3 animate-spin" aria-label="Refreshing MCP health" />
          )}
        </span>
        <div className="flex gap-1">
          {/* Store: no marketplace yet — disabled with an honest label. */}
          <button
            type="button"
            disabled
            className="p-1 rounded text-muted-foreground opacity-40 cursor-not-allowed"
            title="MCP store (coming soon)"
            aria-label="MCP store (coming soon)"
          >
            <ShoppingBag className="w-3.5 h-3.5" />
          </button>
          {/* Settings: close popout and open app Settings → MCP Servers. */}
          <button
            type="button"
            className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-white/[0.05] transition"
            title="MCP settings"
            aria-label="MCP settings"
            onClick={() => {
              onOpenMcpSettings?.()
              onClose()
            }}
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
        {mcpServers.map((server) => {
          // Resolve the dot color from the backend health status when
          // available; otherwise fall back to the legacy enabled-based
          // coloring so the UI doesn't flash muted while the first
          // `getMcpStatus()` is in flight.
          const health = statusByName?.[server.name]
          const dotClass = resolveDotClass(server.enabled, health?.status)
          // Surface the backend error string as a native tooltip on the
          // row when unhealthy — no extra tooltip dependency needed.
          const rowTitle =
            health?.status === 'unhealthy' && health?.error
              ? `${server.name}: ${health.error}`
              : health?.status === 'unhealthy'
                ? `${server.name}: unhealthy`
                : undefined

          return (
          <div
            key={server.name}
            title={rowTitle}
            className={cn(
              'flex items-center justify-between text-[13px] transition-colors',
              server.enabled ? 'text-foreground' : 'text-muted-foreground',
            )}
          >
            <div className="flex items-center gap-2.5">
              {/* Status dot — color driven by backend health (`healthy` /
                  `unhealthy` / `disabled` / `unknown`). Falls back to
                  enabled-based coloring before the first status fetch
                  resolves so there's no muted flash. */}
              <span
                className={cn('w-2 h-2 rounded-full', dotClass)}
                aria-label={
                  health ? `MCP ${health.status}` : server.enabled ? 'MCP enabled' : 'MCP disabled'
                }
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
                aria-label={`Toggle ${server.name}`}
                // Disable ALL toggles while any toggle is in flight (#5) —
                // concurrent toggles race the backend config patch and can
                // leave the list in an inconsistent state. The per-row
                // spinner below still only renders for the active server.
                disabled={togglingServer !== null}
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
          )
        })}
      </div>
    </div>
  )
}

/**
 * Resolve the Tailwind classes for the status dot from the backend health
 * status. When `status` is undefined (no health fetch has resolved yet) or
 * `unknown`, fall back to the legacy enabled-based coloring so enabled
 * servers still show green during the brief loading window rather than
 * flashing muted.
 *
 * Colors follow the project's semantic Tailwind tokens:
 *   healthy   → bg-green-500 with a soft glow
 *   unhealthy → bg-red-500 / text-destructive family with a soft red glow
 *   disabled  → bg-muted-foreground
 *   unknown   → bg-muted-foreground (or enabled-fallback when no status yet)
 */
function resolveDotClass(
  enabled: boolean,
  status: McpHealthStatus | undefined,
): string {
  switch (status) {
    case 'healthy':
      return 'bg-green-500 shadow-[0_0_8px_rgba(16,185,129,0.4)]'
    case 'unhealthy':
      // Red dot with a soft destructive glow; the error reason is shown
      // via the row's native `title` tooltip (set in the map above).
      return 'bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.45)]'
    case 'disabled':
      return 'bg-muted-foreground'
    case 'unknown':
      // Unknown typically means the backend couldn't determine health
      // (e.g. check failed for an enabled server). Show muted so the user
      // sees something is off without implying healthy.
      return 'bg-muted-foreground'
    default:
      // No status entry yet — fall back to enabled-based coloring to
      // avoid a muted flash before the first `getMcpStatus()` resolves.
      return enabled
        ? 'bg-green-500 shadow-[0_0_8px_rgba(16,185,129,0.4)]'
        : 'bg-muted-foreground'
  }
}
