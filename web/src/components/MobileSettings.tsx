import { Wifi, Server, Smartphone, Laptop } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { PairedDevice } from '@/types'

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
  visible,
  onRevokeDevice,
}: {
  devices: PairedDevice[]
  visible: boolean
  onRevokeDevice: (id: string) => void
}) {
  return (
    <div
      className={cn(
        'flex-col absolute inset-0 bg-panel z-30 lg:hidden h-full',
        visible ? 'flex' : 'hidden',
      )}
    >
      <div className="p-3 border-b border-gray-800 shrink-0">
        <h2 className="text-sm font-bold text-gray-400 uppercase tracking-wider">Settings</h2>
      </div>

      <div className="flex-1 overflow-y-auto p-3 space-y-3">
        {/* Connection status */}
        <div className="bg-background border border-gray-800 rounded-lg p-3">
          <div className="text-xs font-semibold text-gray-300 mb-2">Connection</div>
          <div className="flex items-center justify-between text-xs text-gray-400 py-1">
            <span className="flex items-center gap-2"><Wifi className="w-4 h-4 text-green-400" /> Status</span>
            <span className="text-green-400">Online</span>
          </div>
          <div className="flex items-center justify-between text-xs text-gray-400 py-1">
            <span className="flex items-center gap-2"><Server className="w-4 h-4 text-gray-500" /> Daemon</span>
            <span>localhost:7337</span>
          </div>
        </div>

        {/* Paired devices (Blueprint Sec 19 — device pairing) */}
        <div className="bg-background border border-gray-800 rounded-lg p-3">
          <div className="text-xs font-semibold text-gray-300 mb-2">Paired Devices</div>
          {devices.map((d) => {
            const Icon = deviceIconMap[d.icon] ?? Smartphone
            return (
              <div key={d.id} className="flex items-center justify-between text-xs text-gray-400 py-1">
                <span className="flex items-center gap-2">
                  <Icon className="w-4 h-4 text-gray-500" /> {d.name}
                </span>
                <button
                  className="text-red-400 hover:text-red-300"
                  onClick={() => onRevokeDevice(d.id)}
                >
                  Revoke
                </button>
              </div>
            )
          })}
        </div>

        {/* Theme */}
        <div className="bg-background border border-gray-800 rounded-lg p-3">
          <div className="text-xs font-semibold text-gray-300 mb-2">Theme</div>
          <div className="flex items-center gap-2">
            <button className="flex-1 bg-blue-600/20 border border-blue-500/40 text-blue-400 text-xs py-2 rounded-md">
              Dark
            </button>
            <button className="flex-1 bg-gray-800 border border-gray-700 text-gray-400 text-xs py-2 rounded-md">
              Light
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
