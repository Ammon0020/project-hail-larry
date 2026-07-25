import {
  Wrench,
  ShieldAlert,
  ListChecks,
  Terminal,
  FilePen,
  FileSearch,
  Play,
} from 'lucide-react'
import { cva } from 'class-variance-authority'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { AppEvent, StopReason } from '@/types'
import type { PendingPermission } from '@/lib/api'
import { ThinkingBlock } from './chat/ThinkingBlock'
import { ToolExecutionBlock } from './chat/ToolExecutionBlock'
import { ContextInjectionBlock } from './chat/ContextInjectionBlock'

/**
 * Shared Tailwind prose classes for markdown-rendered chat content (code
 * blocks, inline code, links). Used by both the user bubble and the agent
 * StreamUpdate so prose styling stays in sync. Per AGENTS.md, repeated class
 * patterns are extracted rather than duplicated.
 */
const proseClasses =
  'prose prose-sm prose-invert max-w-none [&_pre]:bg-tool-call [&_pre]:rounded-md [&_pre]:border [&_pre]:border-border [&_pre]:p-2 [&_pre]:text-xs [&_pre]:overflow-x-auto [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5 [&_code]:rounded [&_code]:text-xs [&_a]:text-primary [&_a]:hover:underline'

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

/** User chat bubble — right-aligned, slightly lighter than the app bg, with
 *  a sharper bottom-right corner. `error` variant tints with destructive. */
const userBubble = cva(
  'max-w-[85%] break-words rounded-[18px] rounded-br-[4px] px-3 py-2 text-sm text-foreground ' +
    proseClasses,
  {
    variants: { state: { normal: 'bg-secondary', error: 'bg-destructive/20' } },
    defaultVariants: { state: 'normal' },
  },
)

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
        <div className="flex justify-end">
          <div className={userBubble()}>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {event.content || ''}
            </ReactMarkdown>
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
            {event.injectedContext && event.injectedContext.length > 0 && (
              <ContextInjectionBlock context={event.injectedContext} />
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

    case 'ModelChanged':
      // System-level indicator — compact, centered, muted metadata row
      // (same style as ResponseStarted). Shows the model switch without
      // implying history was reset (unlike ConnectionRestarted).
      return (
        <div className="text-xs text-muted-foreground text-center py-1">
          · {event.content || 'Model switched'}
        </div>
      )

    case 'ToolStarted':
      return (
        <div className="flex justify-start">
          <ToolExecutionBlock
            icon={toolKindIcon(event.toolKind)}
            label={event.tool || 'Tool'}
            target={event.target}
            status="[running]"
            command={event.command}
            defaultOpen={false}
          />
        </div>
      )

    case 'ToolCompleted': {
      const failed = event.summary === 'failed'
      return (
        <div className="flex justify-start">
          <ToolExecutionBlock
            label={event.toolKind || event.tool || 'tool'}
            target={event.target}
            status={event.summary || 'completed'}
            failed={failed}
            output={event.content}
            // Auto-expand failed tools so the user sees the error details
            // (e.g. "path outside workspace") without having to click.
            defaultOpen={failed}
          />
        </div>
      )
    }

    case 'PlanUpdated':
      return (
        <div className="flex justify-start">
          <div className="text-xs text-muted-foreground flex items-center gap-1.5">
            <ListChecks className="w-3.5 h-3.5 shrink-0" />
            <span className="text-foreground whitespace-pre-wrap">{event.content}</span>
          </div>
        </div>
      )

    case 'ShellCommandStarted':
      return (
        <div className="flex justify-start">
          <ToolExecutionBlock
            icon={<Terminal className="w-3.5 h-3.5" />}
            label={event.command ? '$ ' + event.command : 'Shell command'}
            status="[running]"
            defaultOpen={false}
          />
        </div>
      )

    case 'ShellCommandCompleted':
      return (
        <div className="flex justify-start">
          <ToolExecutionBlock
            icon={<Terminal className="w-3.5 h-3.5" />}
            label={event.command ? '$ ' + event.command : 'Shell command'}
            status={'exit ' + (event.exitCode ?? '?')}
            failed={event.exitCode !== 0}
            output={event.summary}
            // Auto-expand failed shell commands so the user sees stderr.
            defaultOpen={event.exitCode !== 0}
          />
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
          <div className="flex justify-start">
            <p className="text-xs text-muted-foreground pt-1.5">
              Permission {resolution === 'denied' ? 'denied' : 'granted'} — {toolLabel}
            </p>
          </div>
        )
      }
      // Still pending: render one button per backend-provided option.
      const options = pending?.optionDetails ?? (event.options ?? []).map((id) => ({ id, name: id, kind: id }))
      return (
        <div className="flex justify-start">
          <div className="mt-1 bg-primary/10 border border-primary/30 rounded-lg p-2.5">
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

    case 'StreamUpdate': {
      // Agent thoughts render as a muted, collapsible block.
      if (event.thought) {
        if (!event.content) return null
        return (
          <div className="flex justify-start">
            <ThinkingBlock content={event.content ?? ''} />
          </div>
        )
      }
      if (!event.content && !event.streaming) return null
      // Surface non-normal stop reasons (e.g. max_tokens, refusal, cancelled)
      // subtly below the final assistant message. Normal reasons (end_turn)
      // are hidden by stopReasonLabel.
      const stopLabel = !event.streaming ? stopReasonLabel(event.stopReason) : null
      return (
        <div className="flex justify-start">
          <div className={`break-words text-foreground ${proseClasses}`}>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {event.content || ''}
            </ReactMarkdown>
            {event.streaming && (
              <span className="inline-block w-1.5 h-4 ml-0.5 bg-primary animate-pulse align-text-bottom" />
            )}
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
