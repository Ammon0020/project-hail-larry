import { cva } from 'class-variance-authority'

interface SystemRowProps {
  /** Text body. If omitted, falls back to `fallback`. */
  content?: string
  /** Fallback when content is empty. */
  fallback: string
  /** Optional prefix prepended to content (e.g. "edited ", "Agent exited: "). */
  prefix?: string
  /** destructive variant for failures; default is muted. */
  variant?: 'muted' | 'destructive'
}

const systemRow = cva('text-xs text-center py-1', {
  variants: {
    variant: {
      muted: 'text-muted-foreground',
      destructive: 'text-destructive',
    },
  },
  defaultVariants: { variant: 'muted' },
})

/**
 * Compact centered system-level metadata row used by ResponseStarted,
 * ModelChanged, ConnectionRestarted/SessionResumed, SessionCancelled/
 * SessionInterrupted, FileRevisionUpdated, and AgentExited events. Renders
 * "· {prefix}{content || fallback}". Use `variant="destructive"` for failures.
 */
export function SystemRow({ content, fallback, prefix, variant }: SystemRowProps) {
  return (
    <div className={systemRow({ variant })}>
      · {prefix}{content || fallback}
    </div>
  )
}
