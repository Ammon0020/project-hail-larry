import { Files, Search, Settings } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { LeftPanel } from '@/types'

/**
 * Activity bar — far-left icon-only strip (Blueprint Sec 17).
 * Files, Search at top; connection status and Settings at bottom.
 * Hidden on mobile (mobile uses bottom nav instead).
 */
export function ActivityBar({
  activePanel,
  onSwitchPanel,
  onOpenSettings,
}: {
  activePanel: LeftPanel
  onSwitchPanel: (panel: LeftPanel) => void
  onOpenSettings: () => void
}) {
  const items: { id: LeftPanel; icon: typeof Files; label: string }[] = [
    { id: 'files', icon: Files, label: 'Explorer' },
    { id: 'search', icon: Search, label: 'Search' },
  ]

  return (
    <div className="hidden lg:flex flex-col items-center w-12 bg-activity-bar border-r border-border shrink-0 pt-2 gap-1">
      {items.map(({ id, icon: Icon, label }) => (
        <button
          key={id}
          onClick={() => onSwitchPanel(id)}
          title={label}
          aria-label={label}
          aria-pressed={activePanel === id}
          className={cn(
            'w-full flex items-center justify-center py-2.5 transition relative',
            activePanel === id
              ? 'text-primary hover:text-primary/80'
              : 'text-muted-foreground hover:text-foreground',
          )}
        >
          <Icon className="w-5 h-5" />
          {activePanel === id && (
            <div className="absolute left-0 top-1/2 -translate-y-1/2 w-0.5 h-6 bg-primary rounded-r" />
          )}
        </button>
      ))}

      <div className="flex-1" />

      {/* WebSocket connection status */}
      <div className="flex items-center justify-center py-2" title="Connected to local daemon" role="status" aria-label="Connected to local daemon">
        <div className="w-2 h-2 rounded-full bg-green-400" />
      </div>

      <button
        onClick={onOpenSettings}
        title="Settings"
        aria-label="Settings"
        className="w-full flex items-center justify-center py-2.5 text-muted-foreground hover:text-foreground transition"
      >
        <Settings className="w-5 h-5" />
      </button>
    </div>
  )
}
