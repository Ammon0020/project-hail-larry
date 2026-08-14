import { Files, GitBranch, Search, Settings } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { LeftPanel } from '@/types'

/**
 * Activity bar — far-left icon-only strip (Blueprint Sec 17).
 * Files, Search at top; Settings at bottom.
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
    { id: 'git', icon: GitBranch, label: 'Source Control' },
  ]

  return (
    <div className="hidden w-12 shrink-0 flex-col items-center gap-1 border-r border-border bg-activity-bar pt-2 lg:flex">
      {items.map(({ id, icon: Icon, label }) => (
        <button
          key={id}
          onClick={() => onSwitchPanel(id)}
          title={label}
          aria-label={label}
          aria-pressed={activePanel === id}
          className={cn(
            'relative flex w-full items-center justify-center py-2.5 transition',
            activePanel === id
              ? 'text-primary hover:text-primary/80'
              : 'text-muted-foreground hover:text-foreground',
          )}
        >
          <Icon className="size-5" />
          {activePanel === id && (
            <div className="absolute top-1/2 left-0 h-6 w-0.5 -translate-y-1/2 rounded-r bg-primary" />
          )}
        </button>
      ))}

      <div className="flex-1" />

      <button
        onClick={onOpenSettings}
        title="Settings"
        aria-label="Settings"
        className="flex w-full items-center justify-center py-2.5 text-muted-foreground transition hover:text-foreground"
      >
        <Settings className="size-5" />
      </button>
    </div>
  )
}
