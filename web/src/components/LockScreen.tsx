import { useState, type KeyboardEvent } from 'react'
import { Terminal } from 'lucide-react'
import { api } from '@/lib/api'

/**
 * Lock screen shown to unpaired devices (Blueprint Sec 19 — device pairing).
 * Accepts a four-word mnemonic passcode from `app pair`.
 * Submits the passcode to the daemon's /api/pair/verify-passcode endpoint.
 */
export function LockScreen({ onPaired }: { onPaired: () => void }) {
  const [passcode, setPasscode] = useState('')
  const [error, setError] = useState(false)
  const [errorMsg, setErrorMsg] = useState('')
  const [loading, setLoading] = useState(false)

  /**
   * Validates the passcode format (4 words) and submits to the backend.
   * On success, stores the device credential and calls onPaired.
   */
  const attemptPair = async () => {
    if (loading) return
    const words = passcode.trim().toLowerCase().split(/[\s-]+/).filter(Boolean)
    if (words.length !== 4) {
      setError(true)
      setErrorMsg('Passcode must be 4 words.')
      return
    }

    setLoading(true)
    setError(false)

    try {
      const deviceName = navigator.userAgent.includes('Mobile') ? 'Mobile Device' : 'Browser'
      const cred = await api.verifyPasscode(passcode.trim(), deviceName)
      // Store credential in localStorage (Blueprint Sec 19). Uses the lai: prefix
      // for consistency with other persisted keys (AGENTS.md — consistent keys).
      localStorage.setItem('lai:deviceCredential', JSON.stringify(cred))
      onPaired()
    } catch (err) {
      setError(true)
      setErrorMsg(err instanceof Error ? err.message : 'Invalid or expired passcode.')
    } finally {
      setLoading(false)
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
          <div className="inline-flex items-center justify-center w-16 h-16 rounded-2xl bg-primary/20 border border-primary/30">
            <Terminal className="w-8 h-8 text-primary" />
          </div>
          <h1 className="text-xl font-bold text-foreground">Local Agent Interface</h1>
          <p className="text-sm text-muted-foreground">
            Enter the four-word passcode from{' '}
            <code className="text-primary font-mono">app pair</code>
          </p>
        </div>

        {/* Passcode input */}
        <div className="space-y-3">
          <label htmlFor="passcode-input" className="sr-only">Passcode</label>
          <input
            id="passcode-input"
            type="text"
            value={passcode}
            onChange={(e) => {
              setPasscode(e.target.value)
              setError(false)
            }}
            onKeyDown={handleKeyDown}
            placeholder="purple-fox-delta-wave"
            className="w-full bg-panel border border-input rounded-xl px-4 py-3 text-sm font-mono text-center text-foreground focus:outline-none focus:border-ring focus:ring-1 focus:ring-ring transition"
            autoComplete="off"
            spellCheck="false"
          />
          <button
            onClick={attemptPair}
            disabled={loading}
            className="w-full bg-primary hover:bg-primary/90 disabled:opacity-50 text-primary-foreground font-medium py-3 rounded-xl transition"
          >
            {loading ? 'Pairing...' : 'Pair Device'}
          </button>
          {error && (
            <p className="text-xs text-destructive text-center">{errorMsg}</p>
          )}
        </div>

        {/* Manual connection fallback (Blueprint Sec 20 — network discovery).
            STATUS: stub — the inputs are not yet wired. Shown as a disabled
            "coming soon" affordance so users are not misled into expecting a
            working connect action (AGENTS.md — mark gaps honestly). */}
        <div className="pt-4 border-t border-border">
          <div className="text-xs text-muted-foreground mb-2">Or connect manually:</div>
          <div className="flex gap-2 opacity-50" aria-disabled="true">
            <input
              type="text"
              placeholder="192.168.1.100:7337"
              className="flex-1 bg-panel border border-input rounded-lg px-3 py-2 text-xs font-mono text-foreground focus:outline-none focus:border-ring transition"
              disabled
              aria-label="Manual host and port (coming soon)"
            />
            <button
              className="px-3 py-2 bg-secondary text-secondary-foreground text-xs rounded-lg cursor-not-allowed"
              disabled
              aria-label="Connect manually (coming soon)"
            >
              Connect
            </button>
          </div>
          <p className="text-[10px] text-muted-foreground mt-1.5">Coming soon.</p>
        </div>
      </div>
    </div>
  )
}
