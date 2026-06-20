import { Files, Code, MessageSquare, Settings } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { MobileView } from '@/types'

/**
 * Mobile bottom navigation (Blueprint Sec 17 — mobile layout).
 * One panel at a time: Explorer, Editor, Chat, Settings.
 * Hidden on desktop (lg breakpoint and up).
 */
export function MobileNav({
  activeView,
  onSwitchView,
}: {
  activeView: MobileView
  onSwitchView: (view: MobileView) => void
}) {
  const navItems: {
    view: MobileView
    icon: typeof Files
    label: string
    badge?: string
  }[] = [
    { view: 'explorer', icon: Files,          label: 'Explorer' },
    { view: 'editor',   icon: Code,           label: 'Editor',   badge: 'w-1.5 h-1.5 bg-blue-400' },
    { view: 'chat',     icon: MessageSquare,  label: 'Chat',     badge: 'w-2 h-2 bg-blue-500 animate-pulse' },
    { view: 'settings', icon: Settings,       label: 'Settings' },
  ]

  return (
    <nav className="lg:hidden fixed bottom-0 left-0 w-full h-16 bg-panel border-t border-gray-800 flex items-center justify-around z-50 px-2">
      {navItems.map(({ view, icon: Icon, label, badge }) => (
        <button
          key={view}
          onClick={() => onSwitchView(view)}
          className={cn(
            'flex flex-col items-center justify-center w-16 h-full transition relative',
            activeView === view ? 'text-blue-400' : 'text-gray-500 hover:text-gray-300',
          )}
        >
          {badge && (
            <div className={cn('absolute top-1.5 right-3 rounded-full', badge)} />
          )}
          <Icon className="w-5 h-5 mb-0.5" />
          <span className="text-[10px] font-medium">{label}</span>
        </button>
      ))}
    </nav>
  )
}
