import { Files, Code, MessageSquare, Settings } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { MobileView } from '@/types'

/**
 * Mobile bottom navigation (Blueprint Sec 17 — mobile layout).
 * One panel at a time: Explorer, Editor, Chat, Settings.
 * Settings opens the real SettingsPanel in the editor pane (same as desktop),
 * so it is handled via onOpenSettings rather than a dedicated mobile view.
 * Hidden on desktop (lg breakpoint and up).
 */
export function MobileNav({
  activeView,
  onSwitchView,
  onOpenSettings,
  settingsActive,
}: {
  activeView: MobileView
  onSwitchView: (view: MobileView) => void
  onOpenSettings: () => void
  settingsActive: boolean
}) {
  const navItems: {
    view: MobileView
    icon: typeof Files
    label: string
    badge?: string
  }[] = [
    { view: 'explorer', icon: Files,          label: 'Explorer' },
    { view: 'editor',   icon: Code,           label: 'Editor',   badge: 'w-1.5 h-1.5 bg-primary' },
    { view: 'chat',     icon: MessageSquare,  label: 'Chat',     badge: 'w-2 h-2 bg-primary animate-pulse' },
  ]

  return (
    <nav className="lg:hidden fixed bottom-0 left-0 w-full h-16 bg-panel border-t border-border flex items-center justify-around z-50 px-2">
      {navItems.map(({ view, icon: Icon, label, badge }) => (
        <button
          key={view}
          onClick={() => onSwitchView(view)}
          className={cn(
            'flex flex-col items-center justify-center w-16 h-full transition relative',
            activeView === view && !(view === 'editor' && settingsActive) ? 'text-primary' : 'text-muted-foreground hover:text-foreground',
          )}
        >
          {badge && (
            <div className={cn('absolute top-1.5 right-3 rounded-full', badge)} />
          )}
          <Icon className="w-5 h-5 mb-0.5" />
          <span className="text-[10px] font-medium">{label}</span>
        </button>
      ))}
      <button
        onClick={onOpenSettings}
        className={cn(
          'flex flex-col items-center justify-center w-16 h-full transition relative',
          settingsActive ? 'text-primary' : 'text-muted-foreground hover:text-foreground',
        )}
      >
        <Settings className="w-5 h-5 mb-0.5" />
        <span className="text-[10px] font-medium">Settings</span>
      </button>
    </nav>
  )
}
