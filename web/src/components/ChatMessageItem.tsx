import {
  Bot,
  Wrench,
  ChevronRight,
  ShieldAlert,
  Brain,
  ListChecks,
  Terminal,
  FilePen,
  FileSearch,
  Play,
} from 'lucide-react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { AppEvent, StopReason } from '@/types'
import type { PendingPermission } from '@/lib/api'

/** Picks an icon for a tool card based on the ACP tool kind. */
function toolKindIcon(kind?: string) {
  switch (kind) {
    case 'read':
    case 'search':
      return <FileSearch className="w-3.5 h-3.5 text-blue-400" />
    case 'edit':
    case 'delete':
    case 'move':
      return <FilePen className="w-3.5 h-3.5 text-amber-400" />
    case 'execute':
      return <Play className="w-3.5 h-3.5 text-green-400" />
    default:
      return <Wrench className="w-3.5 h-3.5 text-purple-400" />
  }
}

/** Human label + button style for a permission option kind. */
function optionStyle(kind: string): string {
  if (kind.startsWith('reject') || kind === 'deny') {
    return 'bg-destructive hover:bg-destructive/90'
  }
  return 'bg-primary hover:bg-primary/90'
}

/**
 * Returns true when a string looks like an opaque generated ID rather than a
 * human-readable label. Detects the bug where a raw tool-call ID leaks into the
 * tool field — including IDs that contain `_`/`-`, which the previous heuristic
 * wrongly treated as real labels: e.g. Claude's "toolu_01H…", OpenAI's
 * "call_abc123", UUIDs, long hex tokens, and "muNNhDHjd"-style random tokens.
 * Mirrors the backend heuristic in internal/acp/transport.go#looksLikeRawID.
 */
function looksLikeRawId(value?: string): boolean {
  const v = (value ?? '').trim()
  if (!v) return true
  // Multi-word, human-readable labels (containing whitespace) are never IDs.
  if (/\s/.test(v)) return false
  // Well-known agent tool-call ID prefixes + opaque alphanumeric token.
  if (/^(toolu|tooluse|tool_use|toolcall|call|fc)[_-][A-Za-z0-9]+$/i.test(v)) return true
  // UUID, with or without hyphen separators.
  if (/^[0-9a-f]{8}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{12}$/i.test(v)) return true
  // Long opaque hex token (e.g. SHA-style IDs).
  if (/^[0-9a-f]{16,}$/i.test(v)) return true
  // Separator-free short alphanumeric token (classic random ID shape).
  if (/^[a-zA-Z0-9]{1,24}$/.test(v)) return true
  return false
}

/**
 * Derives a human-readable action label for a permission prompt from the tool
 * kind, used as a fallback when the backend tool field is missing or contains
 * a raw opaque ID.
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
 * backend-supplied tool title; falls back to a kind-derived label when the
 * tool field is missing or contains a raw opaque ID (the "muNNhDHjd" bug).
 */
function permissionToolLabel(tool?: string, toolKind?: string): string {
  if (tool && !looksLikeRawId(tool)) return tool
  return permissionLabelFromKind(toolKind)
}

/**
 * Maps an ACP stop reason to a human-readable label, or returns null when the
 * reason is a normal completion that should not be surfaced to the user.
 *
 * - end_turn: normal — hide.
 * - max_tokens: "hit token limit"
 * - max_turn_requests: "hit turn-request limit"
 * - refusal: "refused"
 * - cancelled: "cancelled"
 * - anything else: shown verbatim so unknown reasons are still visible.
 */
function stopReasonLabel(stopReason?: StopReason): string | null {
  const r = (stopReason ?? '').trim()
  if (!r) return null
  switch (r) {
    case 'end_turn':
      return null
    case 'max_tokens':
      return 'hit token limit'
    case 'max_turn_requests':
      return 'hit turn-request limit'
    case 'refusal':
      return 'refused'
    case 'cancelled':
      return 'cancelled'
    default:
      return r
  }
}

/**
 * Renders a single event from the event stream as a chat message,
 * tool timeline card, permission dialog, plan, or shell card (Blueprint Sec 11).
 *
 * Events arrive over WebSocket as JSON and are rendered chronologically.
 * The UI is derived purely from the event stream.
 */
export function ChatMessageItem({
  event,
  pending,
  resolution,
  onPermissionResponse,
}: {
  event: AppEvent
  /** The matching pending permission (with option details), if still open. */
  pending?: PendingPermission
  /** Resolution of a permission request, derived from grant/deny events. */
  resolution?: 'granted' | 'denied'
  onPermissionResponse?: (requestId: string, decision: string) => void
}) {
  switch (event.type) {
    case 'PromptSubmitted':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-secondary flex items-center justify-center shrink-0 border border-border text-xs font-medium">
            U
          </div>
          <div className="flex-1 pt-0.5 min-w-0">
            <div className="prose prose-sm prose-invert max-w-none break-words text-foreground [&_pre]:bg-tool-call [&_pre]:rounded-md [&_pre]:border [&_pre]:border-border [&_pre]:p-2 [&_pre]:text-xs [&_pre]:overflow-x-auto [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5 [&_code]:rounded [&_code]:text-xs [&_a]:text-primary [&_a]:hover:underline">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {event.content || ''}
              </ReactMarkdown>
            </div>
            {event.attachments && event.attachments.length > 0 && (
              <div className="flex flex-wrap gap-2 mt-2">
                {event.attachments.map((att) => {
                  const src = att.uri ?? `/api/sessions/${event.sessionId}/uploads/${att.id}`
                  return (
                    <a
                      key={att.id}
                      href={src}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="group block rounded-lg border border-border bg-muted p-1.5 hover:border-ring transition"
                      title={att.name}
                    >
                      <img
                        src={src}
                        alt={att.name}
                        className="w-20 h-20 rounded-md object-cover border border-border"
                      />
                      <div className="mt-1 text-[11px] text-muted-foreground truncate max-w-[80px]">
                        {att.name}
                      </div>
                    </a>
                  )
                })}
              </div>
            )}
          </div>
        </div>
      )

    case 'ResponseStarted':
      // System-level indicator — compact, centered, muted metadata row
      // rather than a full chat bubble (Feature 3).
      return (
        <div className="text-xs text-muted-foreground text-center py-1">
          · {event.content || 'Agent is thinking…'}
        </div>
      )

    case 'ToolStarted':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-primary/20 text-primary flex items-center justify-center shrink-0 border border-primary/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <details className="group mt-2 border-l-2 border-border pl-3 open:border-primary/50 transition-colors" open>
              <summary className="flex items-center gap-2 cursor-pointer text-xs font-mono text-muted-foreground hover:text-foreground w-max select-none">
                <ChevronRight className="w-3.5 h-3.5 group-open:rotate-90 transition-transform" />
                {toolKindIcon(event.toolKind)}
                {event.tool || 'Tool'}
                {event.target && <span className="text-foreground">{event.target}</span>}
                <span className="text-muted-foreground">[running]</span>
              </summary>
              {event.command && (
                <pre className="mt-2 bg-tool-call rounded-md border border-border p-2 text-xs whitespace-pre-wrap text-muted-foreground">{event.command}</pre>
              )}
            </details>
          </div>
        </div>
      )

    case 'ToolCompleted': {
      const failed = event.summary === 'failed'
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-primary/20 text-primary flex items-center justify-center shrink-0 border border-primary/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <details className="group mt-2 border-l-2 border-border pl-3 open:border-primary/50 transition-colors">
              <summary className="flex items-center gap-2 cursor-pointer text-xs font-mono text-muted-foreground hover:text-foreground w-max select-none">
                <ChevronRight className="w-3.5 h-3.5 group-open:rotate-90 transition-transform" />
                {toolKindIcon(event.toolKind)}
                {event.toolKind || 'tool'}
                {event.target && <span className="text-foreground">{event.target}</span>}
                <span className={failed ? 'text-destructive' : 'text-muted-foreground'}>[{event.summary || 'completed'}]</span>
              </summary>
              {event.content && (
                <pre className="mt-2 bg-tool-call rounded-md border border-border p-2 text-xs whitespace-pre-wrap text-muted-foreground overflow-x-auto">{event.content}</pre>
              )}
            </details>
          </div>
        </div>
      )
    }

    case 'PlanUpdated':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-primary/20 text-primary flex items-center justify-center shrink-0 border border-primary/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <div className="bg-tool-call border border-border rounded-lg p-2.5 text-xs">
              <div className="text-muted-foreground font-semibold flex items-center gap-1.5 mb-1.5">
                <ListChecks className="w-3.5 h-3.5" /> Plan
              </div>
              <pre className="whitespace-pre-wrap text-foreground font-sans">{event.content}</pre>
            </div>
          </div>
        </div>
      )

    case 'ShellCommandStarted':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-primary/20 text-primary flex items-center justify-center shrink-0 border border-primary/30">
            <Terminal className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <div className="bg-background border border-border rounded-lg px-2.5 py-1.5 text-xs font-mono text-muted-foreground">
              $ {event.command}
            </div>
          </div>
        </div>
      )

    case 'ShellCommandCompleted':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-primary/20 text-primary flex items-center justify-center shrink-0 border border-primary/30">
            <Terminal className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <div className="bg-background border border-border rounded-lg overflow-hidden text-xs font-mono">
              <div className="px-2.5 py-1.5 bg-muted text-muted-foreground flex items-center justify-between">
                <span>$ {event.command}</span>
                <span className={event.exitCode === 0 ? 'text-green-400' : 'text-destructive'}>exit {event.exitCode ?? '?'}</span>
              </div>
              {event.summary && (
                <pre className="p-2.5 text-foreground whitespace-pre-wrap">{event.summary}</pre>
              )}
            </div>
          </div>
        </div>
      )

    case 'ShellOutputStreamed':
      return null

    case 'PermissionRequested': {
      const requestId = event.requestId || ''
      // Display label — prefers the backend tool title, falling back to a
      // kind-derived label when the tool field is missing or is a raw ID.
      const toolLabel = permissionToolLabel(event.tool, event.toolKind)
      // Resolved (answered here or on another device): show the outcome.
      if (resolution) {
        return (
          <div className="flex gap-3">
            <div className="w-7 h-7 rounded-lg bg-primary/20 text-primary flex items-center justify-center shrink-0 border border-primary/30">
              <ShieldAlert className="w-4 h-4" />
            </div>
            <p className="text-xs text-muted-foreground pt-1.5">
              Permission {resolution === 'denied' ? 'denied' : 'granted'} — {toolLabel}
            </p>
          </div>
        )
      }
      // Still pending: render one button per backend-provided option.
      const options = pending?.optionDetails ?? (event.options ?? []).map((id) => ({ id, name: id, kind: id }))
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-primary/20 text-primary flex items-center justify-center shrink-0 border border-primary/30">
            <ShieldAlert className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <div className="mt-2 bg-primary/10 border border-primary/30 rounded-lg p-2.5">
              <div className="flex items-center gap-2 text-xs text-primary font-semibold mb-2">
                <ShieldAlert className="w-3.5 h-3.5" /> Permission Required
              </div>
              <p className="text-xs text-foreground mb-2.5">
                <span className="font-medium">{toolLabel}</span>
                {event.target && <span className="text-muted-foreground"> · {event.target}</span>}
              </p>
              {event.command && (
                <pre className="text-xs font-mono text-muted-foreground bg-background px-1.5 py-1 rounded mb-2.5 whitespace-pre-wrap">{event.command}</pre>
              )}
              {!pending && (
                <p className="text-[11px] text-muted-foreground mb-2">This request is no longer active.</p>
              )}
              <div className="grid grid-cols-2 gap-2">
                {options.map((o) => (
                  <button
                    key={o.id}
                    disabled={!pending}
                    onClick={() => onPermissionResponse?.(requestId, o.id)}
                    className={`text-primary-foreground text-xs font-medium py-1.5 rounded transition disabled:opacity-50 disabled:cursor-not-allowed ${optionStyle(o.kind)}`}
                  >
                    {o.name}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>
      )
    }

    case 'StreamUpdate': {
      // Agent thoughts render as a muted, collapsible block.
      if (event.thought) {
        if (!event.content) return null
        return (
          <div className="flex gap-3">
            <div className="w-7 h-7 rounded-lg bg-primary/20 text-primary flex items-center justify-center shrink-0 border border-primary/30">
              <Brain className="w-4 h-4" />
            </div>
            <details className="flex-1 pt-0.5">
              <summary className="text-xs text-muted-foreground italic cursor-pointer">Thinking…</summary>
              <p className="whitespace-pre-wrap text-xs text-muted-foreground mt-1 pl-1 border-l border-border">{event.content}</p>
            </details>
          </div>
        )
      }
      if (!event.content && !event.streaming) return null
      // Surface non-normal stop reasons (e.g. max_tokens, refusal, cancelled)
      // subtly below the final assistant message. Normal reasons (end_turn)
      // are hidden by stopReasonLabel.
      const stopLabel = !event.streaming ? stopReasonLabel(event.stopReason) : null
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-primary/20 text-primary flex items-center justify-center shrink-0 border border-primary/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5 min-w-0">
            <div className="prose prose-sm prose-invert max-w-none break-words text-foreground [&_pre]:bg-tool-call [&_pre]:rounded-md [&_pre]:border [&_pre]:border-border [&_pre]:p-2 [&_pre]:text-xs [&_pre]:overflow-x-auto [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5 [&_code]:rounded [&_code]:text-xs [&_a]:text-primary [&_a]:hover:underline">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {event.content || ''}
              </ReactMarkdown>
              {event.streaming && (
                <span className="inline-block w-1.5 h-4 ml-0.5 bg-primary animate-pulse align-text-bottom" />
              )}
            </div>
            {stopLabel && (
              <p className="mt-1 text-[11px] text-muted-foreground italic">
                · {stopLabel}
              </p>
            )}
          </div>
        </div>
      )
    }

    case 'AgentExited':
      // System-level failure — compact centered row, but slightly more
      // prominent via text-destructive so failures are noticeable (Feature 3).
      return (
        <div className="text-xs text-destructive text-center py-1">
          · Agent exited: {event.summary || event.content || 'unknown error'}
        </div>
      )

    case 'ConnectionRestarted':
    case 'SessionResumed':
      // System-level metadata — compact centered muted row (Feature 3).
      return (
        <div className="text-xs text-muted-foreground text-center py-1">
          · {event.content || 'Session restarted'}
        </div>
      )

    case 'SessionCancelled':
    case 'SessionInterrupted':
      // System-level metadata — compact centered muted row (Feature 3).
      return (
        <div className="text-xs text-muted-foreground text-center py-1">
          · Stopped
        </div>
      )

    case 'FileRevisionUpdated':
      // System-level metadata — compact centered muted row (Feature 3).
      return (
        <div className="text-xs text-muted-foreground text-center py-1">
          · edited {event.content}
        </div>
      )

    case 'FileWritten':
      // File-write notifications are handled by the file-tree refresh logic
      // in useBackend; they don't render in the chat panel.
      return null

    case 'PermissionGranted':
    case 'PermissionDenied':
      return null

    default:
      return null
  }
}
