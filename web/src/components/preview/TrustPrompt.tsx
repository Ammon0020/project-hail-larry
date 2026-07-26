import { useState } from 'react'
import { ShieldCheck, ShieldAlert } from 'lucide-react'
import { api } from '@/lib/api'
import { cn } from '@/lib/utils'

/**
 * Workspace HTML preview trust prompt.
 *
 * Shown in place of a preview iframe when the workspace's trust state is
 * unknown (`null`/`undefined`). Explains that HTML previews can run scripts
 * and load cross-origin resources (CDNs, APIs), then offers two choices:
 *  - "Trust"        → sets trusted=true  (permissive CSP)
 *  - "Keep untrusted" → sets trusted=false (restrictive CSP, exfil blocked)
 *
 * After either choice, `onResolve` fires with the new trust value so the
 * caller can proceed to render the iframe. Mirrors the error-block styling
 * used by BrowsePreview's `sessionError` state (text-destructive + buttons).
 */
export function TrustPrompt({
  workspaceId,
  onResolve,
  className,
}: {
  workspaceId: string
  /** Called with the chosen trust value once the backend confirms it. */
  onResolve: (trusted: boolean) => void
  className?: string
}) {
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string>()

  async function choose(value: boolean) {
    setSubmitting(true)
    setError(undefined)
    try {
      await api.setWorkspaceTrust(workspaceId, value)
      onResolve(value)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Unable to update trust state')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div
      className={cn(
        'flex flex-col items-center justify-center gap-4 px-6 text-center text-sm',
        className,
      )}
    >
      <ShieldAlert className="w-8 h-8 text-destructive" aria-hidden="true" />
      <p className="max-w-md text-foreground">
        This workspace&apos;s HTML preview can run scripts and load cross-origin
        resources (CDNs, APIs). Trust it?
      </p>
      {error && (
        <p className="text-destructive" role="alert">{error}</p>
      )}
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={() => void choose(true)}
          disabled={submitting}
          className="flex items-center gap-1.5 rounded px-3 py-1.5 font-medium text-primary-foreground bg-primary hover:bg-primary/90 disabled:opacity-50 transition"
        >
          <ShieldCheck className="w-4 h-4" aria-hidden="true" />
          Trust
        </button>
        <button
          type="button"
          onClick={() => void choose(false)}
          disabled={submitting}
          className="flex items-center gap-1.5 rounded px-3 py-1.5 font-medium text-foreground hover:text-primary disabled:opacity-50 transition"
        >
          <ShieldAlert className="w-4 h-4" aria-hidden="true" />
          Keep untrusted
        </button>
      </div>
    </div>
  )
}
