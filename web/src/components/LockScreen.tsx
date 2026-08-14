import { useState, type KeyboardEvent } from 'react'
import { Terminal } from 'lucide-react'
import { api, ApiError } from '@/lib/api'

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
  // Active while a 429 lockout error is shown; submit + Enter are disabled until
  // the user edits the passcode. The backend gives no remaining-seconds value,
  // so we can't render a real countdown — clearing on edit is the honest UX.
  const [locked, setLocked] = useState(false)

  /**
   * Validates the passcode format (4 words) and submits to the backend.
   * On success, stores the device credential and calls onPaired.
   */
  const attemptPair = async () => {
    if (loading || locked) return
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
      // Normalize to the backend's expected form: lowercase, `-`-joined. The
      // server does ct_eq against a `-`-joined lowercase passcode, so sending
      // the raw input rejects valid space/uppercase variations.
      const normalized = words.join('-')
      const cred = await api.verifyPasscode(normalized, deviceName)
      // Store credential in sessionStorage (Blueprint Sec 19). Uses the lai: prefix
      // for consistency with other persisted keys (AGENTS.md — consistent keys).
      sessionStorage.setItem('lai:deviceCredential', JSON.stringify(cred))
      onPaired()
    } catch (err) {
      setError(true)
      if (err instanceof ApiError && err.status === 429) {
        setLocked(true)
        setErrorMsg('Too many attempts — wait a moment, then edit the passcode and try again.')
      } else {
        setErrorMsg(err instanceof Error ? err.message : 'Invalid or expired passcode.')
      }
    } finally {
      setLoading(false)
    }
  }

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') attemptPair()
  }

  return (
    <div className="flex size-full flex-col items-center justify-center bg-background px-4">
      <div className="w-full max-w-sm space-y-6">
        {/* Logo / title */}
        <div className="space-y-2 text-center">
          <div className="inline-flex size-16 items-center justify-center rounded-2xl border border-primary/30 bg-primary/20">
            <Terminal className="size-8 text-primary" />
          </div>
          <h1 className="text-xl font-bold text-foreground">Local Agent Interface</h1>
          <p className="text-sm text-muted-foreground">
            Enter the four-word passcode from{' '}
            <code className="font-mono text-primary">app pair</code>
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
              setLocked(false)
            }}
            onKeyDown={handleKeyDown}
            placeholder="purple-fox-delta-wave"
            className="w-full rounded-xl border border-input bg-panel px-4 py-3 text-center font-mono text-sm text-foreground transition focus:border-ring focus:ring-1 focus:ring-ring focus:outline-none"
            autoComplete="off"
            spellCheck="false"
          />
          <button
            onClick={attemptPair}
            disabled={loading || locked}
            className="w-full rounded-xl bg-primary py-3 font-medium text-primary-foreground transition hover:bg-primary/90 disabled:opacity-50"
          >
            {loading ? 'Pairing...' : locked ? 'Locked — edit passcode' : 'Pair Device'}
          </button>
          {error && (
            <p className="text-center text-xs text-destructive">{errorMsg}</p>
          )}
        </div>

        {/* Manual connection fallback (Blueprint Sec 20 — network discovery).
            STATUS: stub — the inputs are not yet wired. Shown as a disabled
            "coming soon" affordance so users are not misled into expecting a
            working connect action (AGENTS.md — mark gaps honestly). */}
        <div className="border-t border-border pt-4">
          <div className="mb-2 text-xs text-muted-foreground">Or connect manually:</div>
          <div className="flex gap-2 opacity-50" aria-disabled="true">
            <input
              type="text"
              placeholder="192.168.1.100:7337"
              className="flex-1 rounded-lg border border-input bg-panel px-3 py-2 font-mono text-xs text-foreground transition focus:border-ring focus:outline-none"
              disabled
              aria-label="Manual host and port (coming soon)"
            />
            <button
              className="cursor-not-allowed rounded-lg bg-secondary px-3 py-2 text-xs text-secondary-foreground"
              disabled
              aria-label="Connect manually (coming soon)"
            >
              Connect
            </button>
          </div>
          <p className="mt-1.5 text-[10px] text-muted-foreground">Coming soon.</p>
        </div>
      </div>
    </div>
  )
}
