import { ShieldAlert } from 'lucide-react'
import type { PendingPermission } from '@/lib/api'

interface PermissionCardProps {
  requestId: string
  tool?: string
  toolKind?: string
  target?: string
  command?: string
  /** Raw event.options fallback when no pending option details are available. */
  options?: string[]
  pending?: PendingPermission
  resolution?: 'granted' | 'denied'
  onPermissionResponse?: (requestId: string, decision: string) => void
}

/** Human label + button style for a permission option kind. */
function optionStyle(kind: string): string {
  if (kind.startsWith('reject') || kind === 'deny') {
    return 'bg-destructive hover:bg-destructive/90'
  }
  return 'bg-primary hover:bg-primary/90'
}

/**
 * Derives a human-readable action label for a permission prompt from the tool
 * kind, used as a fallback when the backend tool field is missing.
 *
 * The backend (internal/acp/transport.go#RequestPermission) authoritatively
 * sanitizes the tool title before emitting the event: it replaces any opaque
 * tool-call ID (e.g. "toolu_01H…", "call_abc123", UUIDs, "muNNhDHjd") with a
 * kind-derived label, so the frontend can trust `event.tool` and does not need
 * to re-run the raw-ID heuristic. This fallback only covers a missing/empty
 * tool field.
 */
function permissionLabelFromKind(kind?: string): string {
  switch (kind) {
    case 'execute':
      return 'Run command'
    case 'edit':
      return 'Edit file'
    case 'read':
      return 'Read file'
    case 'search':
      return 'Search'
    case 'delete':
      return 'Delete file'
    case 'move':
      return 'Move file'
    default:
      return 'Tool call'
  }
}

/**
 * Resolves the display label for a permission request event. Prefers the
 * backend-supplied tool title (already sanitized of raw IDs by the backend);
 * falls back to a kind-derived label only when the tool field is missing.
 */
function permissionToolLabel(tool?: string, toolKind?: string): string {
  if (tool) return tool
  return permissionLabelFromKind(toolKind)
}

/**
 * Permission prompt card. Renders a "Permission Required" panel with one
 * button per backend-provided option while pending, or a compact inline
 * "Permission granted/denied — <tool>" row once resolved. The `reject_once`
 * option kind is normalized to `deny` in the onClick handler.
 */
export function PermissionCard({
  requestId,
  tool,
  toolKind,
  target,
  command,
  options,
  pending,
  resolution,
  onPermissionResponse,
}: PermissionCardProps) {
  const toolLabel = permissionToolLabel(tool, toolKind)
  // Resolved (answered here or on another device): show the outcome.
  if (resolution) {
    return (
      <div className="flex justify-start">
        <p className="text-xs text-muted-foreground pt-1.5">
          Permission {resolution === 'denied' ? 'denied' : 'granted'} — {toolLabel}
        </p>
      </div>
    )
  }
  // Still pending: render one button per backend-provided option.
  const opts = pending?.optionDetails ?? (options ?? []).map((id) => ({ id, name: id, kind: id }))
  return (
    <div className="flex justify-start">
      <div
        role="alert"
        aria-live="assertive"
        aria-atomic="true"
        className="mt-1 bg-primary/10 border border-primary/30 rounded-lg p-2.5"
      >
        <div className="flex items-center gap-2 text-xs text-primary font-semibold mb-2">
          <ShieldAlert className="w-3.5 h-3.5" /> Permission Required
        </div>
        <p className="text-xs text-foreground mb-2.5">
          <span className="font-medium">{toolLabel}</span>
          {target && <span className="text-muted-foreground"> · {target}</span>}
        </p>
        {command && (
          <pre className="text-xs font-mono text-muted-foreground bg-background px-1.5 py-1 rounded mb-2.5 whitespace-pre-wrap">{command}</pre>
        )}
        {!pending && (
          <p className="text-[11px] text-muted-foreground mb-2">This request is no longer active.</p>
        )}
        <div className="grid grid-cols-2 gap-2">
          {opts.map((o) => (
            <button
              key={o.id}
              disabled={!pending}
              onClick={() => {
                let decision = o.kind;
                if (decision === 'reject_once') decision = 'deny';
                onPermissionResponse?.(requestId, decision);
              }}
              className={`text-primary-foreground text-xs font-medium py-1.5 rounded transition disabled:opacity-50 disabled:cursor-not-allowed ${optionStyle(o.kind)}`}
            >
              {o.name}
            </button>
          ))}
        </div>
      </div>
    </div>
  )
}
