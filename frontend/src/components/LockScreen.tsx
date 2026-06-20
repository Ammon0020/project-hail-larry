import { useState, type KeyboardEvent } from 'react'
import { Terminal } from 'lucide-react'

/**
 * Lock screen shown to unpaired devices (Blueprint Sec 19 — device pairing).
 * Accepts a four-word mnemonic passcode from `app pair`.
 * In production, this submits a one-time token to the daemon.
 */
export function LockScreen({ onPaired }: { onPaired: () => void }) {
  const [passcode, setPasscode] = useState('')
  const [error, setError] = useState(false)

  /** Validates the passcode format (4 words separated by hyphens or spaces). */
  const attemptPair = () => {
    const words = passcode.trim().toLowerCase().split(/[\s-]+/).filter(Boolean)
    if (words.length === 4) {
      onPaired()
    } else {
      setError(true)
    }
  }

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') attemptPair()
  }

  return (
    <div className="flex flex-col items-center justify-center h-full w-full bg-background px-4">
      <div className="w-full max-w-sm space-y-6">
        {/* Logo / title */}
        <div className="text-center space-y-2">
          <div className="inline-flex items-center justify-center w-16 h-16 rounded-2xl bg-blue-600/20 border border-blue-500/30">
            <Terminal className="w-8 h-8 text-blue-400" />
          </div>
          <h1 className="text-xl font-bold text-gray-100">Local Agent Interface</h1>
          <p className="text-sm text-gray-500">
            Enter the four-word passcode from{' '}
            <code className="text-blue-400 font-mono">app pair</code>
          </p>
        </div>

        {/* Passcode input */}
        <div className="space-y-3">
          <input
            type="text"
            value={passcode}
            onChange={(e) => {
              setPasscode(e.target.value)
              setError(false)
            }}
            onKeyDown={handleKeyDown}
            placeholder="purple-fox-delta-wave"
            className="w-full bg-panel border border-gray-700 rounded-xl px-4 py-3 text-sm font-mono text-center text-gray-200 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 transition"
            autoComplete="off"
            spellCheck="false"
          />
          <button
            onClick={attemptPair}
            className="w-full bg-blue-600 hover:bg-blue-500 text-white font-medium py-3 rounded-xl transition"
          >
            Pair Device
          </button>
          {error && (
            <p className="text-xs text-red-400 text-center">Invalid passcode. Try again.</p>
          )}
        </div>

        {/* Manual connection fallback (Blueprint Sec 20 — network discovery) */}
        <div className="pt-4 border-t border-gray-800">
          <div className="text-xs text-gray-500 mb-2">Or connect manually:</div>
          <div className="flex gap-2">
            <input
              type="text"
              placeholder="192.168.1.100:7337"
              className="flex-1 bg-panel border border-gray-700 rounded-lg px-3 py-2 text-xs font-mono text-gray-300 focus:outline-none focus:border-blue-500 transition"
            />
            <button className="px-3 py-2 bg-gray-800 hover:bg-gray-700 text-gray-300 text-xs rounded-lg transition">
              Connect
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
