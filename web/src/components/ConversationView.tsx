import { useMemo } from 'react'
import { ChevronDown, X } from 'lucide-react'
import { ChatMessageItem } from './ChatMessageItem'
import { Banner } from './ui/Banner'
import type { AppEvent } from '@/types'
import type { PendingPermission } from '@/lib/api'

interface ConversationViewProps {
  /** Already-merged events (consecutive StreamUpdates collapsed). */
  events: AppEvent[]
  pendingPermissions: PendingPermission[]
  permissionResolution: Map<string, 'granted' | 'denied'>
  onPermissionResponse: (id: string, decision: string) => void
  error: string | null
  /** Stable ref owned by ChatPanel so autoscroll survives slot swaps. */
  scrollContainerRef: React.RefObject<HTMLDivElement | null>
  isAtBottom: boolean
  onJumpToBottom: () => void
  /** Whether another older event page can be loaded for this conversation. */
  hasOlderEvents: boolean
  loadingOlderEvents: boolean
  onLoadOlder: () => void
  /** When true, shows a "MCP config changed — restart to apply" banner. */
  mcpConfigChanged?: boolean
  /** Dismisses the MCP config changed banner. */
  onDismissMcpBanner?: () => void
  /** Restarts the session to apply MCP config changes. */
  onRestartForMcp?: () => void
}

/**
 * Default message-area slot implementation. Renders the merged event stream
 * via `ChatMessageItem`, an empty-state hint, an error banner, and a
 * jump-to-bottom button. The scroll container ref + isAtBottom + jump callback
 * are owned by `ChatPanel` (the orchestrator) so the autoscroll contract stays
 * stable across slot swaps (design §1).
 */
export function ConversationView({
  events,
  pendingPermissions,
  permissionResolution,
  onPermissionResponse,
  error,
  scrollContainerRef,
  isAtBottom,
  onJumpToBottom,
  hasOlderEvents,
  loadingOlderEvents,
  onLoadOlder,
  mcpConfigChanged,
  onDismissMcpBanner,
  onRestartForMcp,
}: ConversationViewProps) {
  // Group events into turns: each user prompt starts a new turn, and all
  // subsequent agent events fold into the same agent turn until the next
  // prompt. User turns render as a single bubble; agent turns get tight
  // `space-y-1` spacing so the trace reads as one block (design §2).
  const turns: Array<{ kind: 'user' | 'agent'; events: AppEvent[] }> = useMemo(() => {
    const out: Array<{ kind: 'user' | 'agent'; events: AppEvent[] }> = []
    for (const ev of events) {
      if (ev.type === 'PromptSubmitted') out.push({ kind: 'user', events: [ev] })
      else {
        const last = out[out.length - 1]
        if (last && last.kind === 'agent') last.events.push(ev)
        else out.push({ kind: 'agent', events: [ev] })
      }
    }
    return out
  }, [events])

  return (
    <div className="relative flex-1 min-h-0">
      <div
        ref={scrollContainerRef}
        className="h-full overflow-y-auto p-3 lg:p-4 pb-20 lg:pb-4"
      >
        {events.length === 0 && !error && (
          <div className="rounded-lg border border-border bg-panel/50 p-3 text-xs text-muted-foreground">
            Send a message to start a conversation.
          </div>
        )}
        {hasOlderEvents && (
          <div className="mb-3 flex justify-center">
            <button
              type="button"
              onClick={onLoadOlder}
              disabled={loadingOlderEvents}
              className="rounded border border-border px-3 py-1 text-xs text-muted-foreground hover:bg-accent disabled:cursor-wait disabled:opacity-60"
            >
              {loadingOlderEvents ? 'Loading older messages…' : 'Load older messages'}
            </button>
          </div>
        )}
        {turns.map((turn, ti) => {
          const inner = turn.events.map((event, ei) => (
            <ChatMessageItem
              key={event.id ?? `${event.type}-${ti}-${ei}`}
              event={event}
              pending={
                event.type === 'PermissionRequested' && event.requestId
                  ? pendingPermissions.find((p) => p.id === event.requestId)
                  : undefined
              }
              resolution={
                event.type === 'PermissionRequested' && event.requestId
                  ? permissionResolution.get(event.requestId)
                  : undefined
              }
              onPermissionResponse={onPermissionResponse}
            />
          ))
          return turn.kind === 'user' ? (
            <div key={`turn-${ti}`} className="mb-3">
              {inner}
            </div>
          ) : (
            <div key={`turn-${ti}`} className="space-y-1 mb-3">
              {inner}
            </div>
          )
        })}
        {mcpConfigChanged && (
          <Banner
            variant="success"
            className="rounded-lg border p-3 flex items-center justify-between gap-2"
          >
            <span>MCP config changed — restart to apply</span>
            <div className="flex items-center gap-2">
              {onRestartForMcp && (
                <button
                  onClick={onRestartForMcp}
                  className="font-medium bg-primary text-primary-foreground px-2 py-0.5 rounded hover:bg-primary/90 transition"
                >
                  Restart
                </button>
              )}
              {onDismissMcpBanner && (
                <button
                  onClick={onDismissMcpBanner}
                  className="text-primary/70 hover:text-primary transition"
                  aria-label="Dismiss"
                >
                  <X className="w-3 h-3" />
                </button>
              )}
            </div>
          </Banner>
        )}
        {error && (
          <Banner
            variant="error"
            className="rounded-lg border p-3"
          >
            {error}
          </Banner>
        )}
      </div>

      {/* Jump-to-bottom — shown only when the user has scrolled away. */}
      {!isAtBottom && (
        <button
          onClick={onJumpToBottom}
          className="absolute bottom-4 right-4 rounded-full bg-background border border-border p-2 shadow-md hover:bg-accent text-muted-foreground hover:text-foreground transition"
          title="Jump to bottom"
          aria-label="Jump to bottom"
        >
          <ChevronDown className="w-4 h-4" />
        </button>
      )}
    </div>
  )
}
