import {
  Wrench,
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
import { PermissionCard } from './chat/PermissionCard'
import { SystemRow } from './chat/SystemRow'

/**
 * Shared Tailwind prose classes for markdown-rendered chat content (code
 * blocks, inline code, links). Used by both the user bubble and the agent
 * StreamUpdate so prose styling stays in sync. Per AGENTS.md, repeated class
 * patterns are extracted rather than duplicated.
 */
const proseClasses =
  'prose prose-sm dark:prose-invert max-w-none [&_pre]:bg-tool-call [&_pre]:rounded-md [&_pre]:border [&_pre]:border-border [&_pre]:p-2 [&_pre]:text-xs [&_pre]:overflow-x-auto [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5 [&_code]:rounded [&_code]:text-xs [&_a]:text-primary [&_a]:hover:underline'

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
      return <SystemRow content={event.content} fallback="Agent is thinking…" />

    case 'ModelChanged':
      // System-level indicator — compact, centered, muted metadata row
      // (same style as ResponseStarted). Shows the model switch without
      // implying history was reset (unlike ConnectionRestarted).
      return <SystemRow content={event.content} fallback="Model switched" />

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
      return (
        <PermissionCard
          requestId={requestId}
          tool={event.tool}
          toolKind={event.toolKind}
          target={event.target}
          command={event.command}
          options={event.options}
          pending={pending}
          resolution={resolution}
          onPermissionResponse={onPermissionResponse}
        />
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
        <SystemRow
          content={event.summary || event.content}
          fallback="unknown error"
          prefix="Agent exited: "
          variant="destructive"
        />
      )

    case 'ConnectionRestarted':
    case 'SessionResumed':
      // System-level metadata — compact centered muted row (Feature 3).
      return <SystemRow content={event.content} fallback="Session restarted" />

    case 'SessionCancelled':
    case 'SessionInterrupted':
      // System-level metadata — compact centered muted row (Feature 3).
      return <SystemRow fallback="Stopped" />

    case 'FileRevisionUpdated':
      // System-level metadata — compact centered muted row (Feature 3).
      return <SystemRow content={event.content} fallback="" prefix="edited " />

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
