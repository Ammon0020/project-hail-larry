import { Wifi, Server, Smartphone, Laptop } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useTheme } from '@/hooks/useTheme'
import type { Theme } from '@/lib/theme'
import type { PairedDevice } from '@/types'
import type { AgentInfo } from '@/lib/api'

/** Selectable theme options for the settings toggle. */
const THEME_OPTIONS: { value: Theme; label: string }[] = [
  { value: 'dark', label: 'Dark' },
  { value: 'light', label: 'Light' },
  { value: 'system', label: 'System' },
]

/** Maps device icon names to Lucide React components. */
const deviceIconMap: Record<string, typeof Smartphone> = {
  smartphone: Smartphone,
  laptop: Laptop,
}

/**
 * Mobile settings panel (Blueprint Sec 20 — configuration).
 * Shows connection status, paired devices, and theme toggle.
 * Full-screen overlay on mobile only.
 */
export function MobileSettings({
  devices,
  agents,
  visible,
  onRevokeDevice,
  onAutodetectAgents,
}: {
  devices: PairedDevice[]
  agents: AgentInfo[]
  visible: boolean
  onRevokeDevice: (id: string) => void
  onAutodetectAgents: () => void
}) {
  const { theme, setTheme } = useTheme()
  return (
    <div
      className={cn(
        'flex-col absolute inset-0 bg-panel z-30 lg:hidden h-full',
        visible ? 'flex' : 'hidden',
      )}
    >
      <div className="p-3 border-b border-border shrink-0">
        <h2 className="text-sm font-bold text-muted-foreground uppercase tracking-wider">Settings</h2>
      </div>

      <div className="flex-1 overflow-y-auto p-3 space-y-3">
        {/* Connection status */}
        <div className="bg-background border border-border rounded-lg p-3">
          <div className="text-xs font-semibold text-foreground mb-2">Connection</div>
          <div className="flex items-center justify-between text-xs text-muted-foreground py-1">
            <span className="flex items-center gap-2"><Wifi className="w-4 h-4 text-green-400" /> Status</span>
            <span className="text-green-400">Online</span>
          </div>
          <div className="flex items-center justify-between text-xs text-muted-foreground py-1">
            <span className="flex items-center gap-2"><Server className="w-4 h-4 text-muted-foreground" /> Daemon</span>
            <span>localhost:7337</span>
          </div>
        </div>

        {/* Paired devices (Blueprint Sec 19 — device pairing) */}
        <div className="bg-background border border-border rounded-lg p-3">
          <div className="text-xs font-semibold text-foreground mb-2">Paired Devices</div>
          {devices.map((d) => {
            const Icon = deviceIconMap[d.icon] ?? Smartphone
            return (
              <div key={d.id} className="flex items-center justify-between text-xs text-muted-foreground py-1">
                <span className="flex items-center gap-2">
                  <Icon className="w-4 h-4 text-muted-foreground" /> {d.name}
                </span>
                <button
                  className="text-destructive hover:text-destructive/80"
                  onClick={() => onRevokeDevice(d.id)}
                >
                  Revoke
                </button>
              </div>
            )
          })}
        </div>

        {/* Agents */}
        <div className="bg-background border border-border rounded-lg p-3">
          <div className="flex items-center justify-between mb-2">
            <div className="text-xs font-semibold text-foreground">Agents</div>
            <button onClick={onAutodetectAgents} className="text-[10px] text-primary border border-primary/30 px-2 py-0.5 rounded hover:bg-primary/10 transition">Auto-detect</button>
          </div>
          {agents.length === 0 ? (
            <div className="text-xs text-muted-foreground py-1">No agents configured.</div>
          ) : (
            agents.map(a => (
              <div key={a.id} className="flex items-center justify-between text-xs text-muted-foreground py-1">
                <span>{a.name} <span className="text-[10px] text-muted-foreground bg-muted px-1 rounded ml-1">{a.command}</span></span>
              </div>
            ))
          )}
        </div>

        {/* Theme */}
        <div className="bg-background border border-border rounded-lg p-3">
          <div className="text-xs font-semibold text-foreground mb-2">Theme</div>
          <div className="flex items-center gap-2">
            {THEME_OPTIONS.map((opt) => {
              const active = theme === opt.value
              return (
                <button
                  key={opt.value}
                  onClick={() => setTheme(opt.value)}
                  aria-pressed={active}
                  className={cn(
                    'flex-1 border text-xs py-2 rounded-md transition-colors',
                    active
                      ? 'bg-primary/15 border-primary/50 text-primary'
                      : 'bg-secondary border-border text-muted-foreground hover:text-foreground',
                  )}
                >
                  {opt.label}
                </button>
              )
            })}
          </div>
        </div>
      </div>
    </div>
  )
}
