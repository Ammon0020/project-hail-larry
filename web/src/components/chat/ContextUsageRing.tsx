import { useEffect, useRef, useState } from 'react'
import { cn } from '@/lib/utils'
import {
  type ContextUsage,
  contextFillFraction,
  contextFillPercent,
  formatTokens,
  formatCost,
} from '@/lib/contextUsage'

interface ContextUsageRingProps {
  /** Latest context usage for the active session, or null when no
   *  `UsageUpdated` event has been received yet. When null, a faint dashed
   *  placeholder ring is shown so the indicator is always discoverable. */
  usage: ContextUsage | null
  /** True while the agent is running a turn — brightens the placeholder
   *  ring so the user can see the indicator is live during a turn. */
  active: boolean
}

/**
 * Context-usage ring — a standalone 28px circle (same size as the send
 * button) placed to its left in the chat composer actions row.
 *
 * Always visible. Renders an SVG ring that fills clockwise as the agent's
 * context window fills, based on ACP `usage_update` events. A hover/tap
 * popout shows the percentage, token count, and cumulative cost.
 *
 * Two visual states:
 *  - **No data yet** (`usage` is null): a faint dashed placeholder ring so
 *    the user can see the indicator exists even before the agent reports
 *    usage. Brightens slightly while a turn is active.
 *  - **Data flowing** (`usage` is non-null): a solid fill ring that grows
 *    clockwise. Color shifts muted → primary → destructive at 50% / 90%.
 *
 * The popout follows the McpPopout pattern: outside-click and Escape close
 * it, and it is anchored above the ring via `absolute bottom-full`.
 */
export function ContextUsageRing({ usage, active }: ContextUsageRingProps) {
  const [showPopout, setShowPopout] = useState(false)
  const popoutRef = useRef<HTMLDivElement>(null)
  const ringRef = useRef<HTMLDivElement>(null)

  // Close on outside-click and Escape, following the McpPopout pattern.
  // The ring trigger is marked with `data-usage-ring` so its own clicks
  // don't close the popout it just opened.
  useEffect(() => {
    if (!showPopout) return
    function handleClickOutside(e: MouseEvent) {
      const target = e.target as Element | null
      if (!target) return
      if (popoutRef.current?.contains(target)) return
      if (typeof target.closest === 'function' && target.closest('[data-usage-ring]')) {
        return
      }
      setShowPopout(false)
    }
    function handleEscape(e: KeyboardEvent) {
      if (e.key === 'Escape') setShowPopout(false)
    }
    document.addEventListener('mousedown', handleClickOutside)
    document.addEventListener('keydown', handleEscape)
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
      document.removeEventListener('keydown', handleEscape)
    }
  }, [showPopout])

  const fraction = contextFillFraction(usage)
  const hasUsage = usage !== null

  // Ring geometry — ~2/3 the size of the send button (28px → 19px).
  const ringSize = 19
  const stroke = 1.5
  const radius = (ringSize - stroke) / 2
  const circumference = 2 * Math.PI * radius
  // Fill clockwise from the top: strokeDasharray + strokeDashoffset.
  const dashOffset = circumference * (1 - fraction)

  // Color shifts from muted → primary → destructive as the window fills.
  const ringColor =
    fraction >= 0.9
      ? 'stroke-destructive'
      : fraction >= 0.5
        ? 'stroke-primary'
        : 'stroke-muted-foreground'

  return (
    <div
      ref={ringRef}
      data-usage-ring
      className="relative shrink-0 cursor-pointer"
      onMouseEnter={() => setShowPopout(true)}
      onMouseLeave={() => setShowPopout(false)}
      onClick={() => setShowPopout((v) => !v)}
      title={hasUsage ? contextFillPercent(usage) : 'Context usage (waiting for agent)'}
    >
      <svg
        width={ringSize}
        height={ringSize}
        viewBox={`0 0 ${ringSize} ${ringSize}`}
        aria-hidden="true"
      >
        {/* Track — always present as the base ring. Dashed when no data,
            solid faint when data is flowing. */}
        <circle
          cx={ringSize / 2}
          cy={ringSize / 2}
          r={radius}
          fill="none"
          strokeWidth={stroke}
          className={hasUsage ? 'stroke-border' : cn('stroke-muted-foreground/50', active && 'stroke-muted-foreground/70')}
          strokeDasharray={hasUsage ? undefined : '6 7'}
        />
        {/* Fill — clockwise from 12 o'clock via dashoffset + transform.
            Only rendered once we have real usage data; the dashed track
            above serves as the placeholder when no data has arrived. */}
        {hasUsage && (
          <circle
            cx={ringSize / 2}
            cy={ringSize / 2}
            r={radius}
            fill="none"
            strokeWidth={stroke}
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={dashOffset}
            className={cn(ringColor, 'transition-all duration-300')}
            transform={`rotate(-90 ${ringSize / 2} ${ringSize / 2})`}
          />
        )}
      </svg>

      {/* Popout — anchored above the ring, following the McpPopout pattern.
          Shown on hover/tap; content depends on whether we have data. */}
      {showPopout && (
        <div
          ref={popoutRef}
          className="absolute bottom-full right-0 mb-2 w-[200px] z-50 bg-popover border border-border rounded-[10px] shadow-lg p-3 flex flex-col gap-1.5"
        >
          {hasUsage ? (
            <>
              <div className="text-xs font-medium text-foreground">
                {contextFillPercent(usage)} context used
              </div>
              <div className="text-[11px] text-muted-foreground">
                {formatTokens(usage.used)} / {formatTokens(usage.size)} tokens
              </div>
              {usage.costAmount !== undefined && (
                <div className="text-[11px] text-muted-foreground">
                  {formatCost(usage.costAmount, usage.costCurrency)}
                </div>
              )}
            </>
          ) : (
            <div className="text-[11px] text-muted-foreground">
              Context usage will appear here once the agent reports it.
            </div>
          )}
        </div>
      )}
    </div>
  )
}
