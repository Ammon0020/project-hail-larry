/**
 * Converts our append-only `AppEvent` stream (received over WebSocket from the
 * Rust daemon) into assistant-ui's `ThreadMessageLike[]` so the conversation
 * can be rendered by `ThreadPrimitive` with the library's clean styling.
 *
 * The event stream is already merged by `ChatPanel.mergedEvents` before it
 * reaches us: consecutive `StreamUpdate` chunks of the same role/thought kind
 * are folded into one, `ShellOutputStreamed` chunks are folded into their
 * `ShellCommandStarted` card, and `ShellCommandCompleted` replaces the started
 * card in place. We only need to group the merged events into messages and
 * map each onto assistant-ui content parts.
 *
 * Mapping summary (preserves 1:1 functionality with the prior ChatMessageItem):
 * - PromptSubmitted            → user message (text + image parts + custom
 *                                context-injection part carried via metadata)
 * - ResponseStarted            → system message ("Agent is thinking…")
 * - StreamUpdate (text)        → assistant message text part (markdown)
 * - StreamUpdate (thought)     → assistant message reasoning part
 * - ToolStarted / ToolCompleted→ assistant message tool-call part
 * - ShellCommandStarted/Comp.  → assistant message tool-call part (shell)
 * - PermissionRequested        → assistant message tool-call part with
 *                                approval state (pending/resolved) so the
 *                                built-in ToolFallback.Approval UI can render
 *                                grant/deny buttons; resolution is patched in
 *                                from the `permissionResolution` map.
 * - PlanUpdated                → assistant message custom data part ("plan")
 * - ModelChanged, ConnectionRestarted, SessionResumed, SessionInterrupted,
 *   SessionCancelled, AgentExited, FileRevisionUpdated, FileChangedOnDisk,
 *   SessionCreated, SessionClosed → system message
 * - FileWritten, PermissionGranted, PermissionDenied, ShellOutputStreamed →
 *   dropped (handled elsewhere or folded by mergedEvents).
 *
 * Grouping: a user prompt starts a new user message. Each subsequent agent
 * event becomes its own assistant message so the timeline reads the same as
 * before (tool cards, thoughts, and text chunks are visually distinct rows).
 * Consecutive same-kind agent events that the old UI rendered as one growing
 * block (e.g. streaming text) are already merged upstream, so we keep one
 * event = one message for agent rows. This matches the prior turn grouping
 * where agent events were laid out with `space-y-1`.
 */
import type { ThreadMessageLike } from '@assistant-ui/react'
import type { AppEvent, Attachment, StopReason } from '@/types'
import type { PendingPermission } from '@/lib/api'

/**
 * Custom content part types carried in the `content` array via the
 * `data-${name}` shape that ThreadMessageLike allows. Used for parts that
 * don't have a native assistant-ui representation (context injection, plan,
 * shell command metadata). The part renderers in `AssistantThread` switch on
 * these `type` strings.
 */
export interface ContextInjectionPart {
  readonly type: 'data-context-injection'
  readonly context: { name: string; content: string }[]
}

export interface PlanPart {
  readonly type: 'data-plan'
  readonly content: string
}

export interface SystemRowPart {
  readonly type: 'data-system-row'
  readonly content?: string
  readonly fallback: string
  readonly prefix?: string
  readonly variant?: 'muted' | 'destructive'
}

/**
 * Shape of the per-message metadata we attach to assistant messages so the
 * tool-call part renderer can recover the original event fields (toolKind,
 * command, exitCode, summary, target) that assistant-ui's ToolCallMessagePart
 * doesn't carry natively.
 */
export interface ToolCallMetadata {
  toolKind?: string
  target?: string
  command?: string
  summary?: string
  exitCode?: number
  /** True for shell-command tool calls (drives the Terminal icon). */
  isShell?: boolean
}

/** Maps an ACP stop reason to a human-readable label, or null for normal ends. */
export function stopReasonLabel(stopReason?: StopReason): string | null {
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

/** Converts an `Attachment` to an assistant-ui image content part. */
function attachmentToImagePart(att: Attachment) {
  return {
    type: 'image' as const,
    image: att.uri ?? '',
    filename: att.name,
  }
}

/**
 * Mutable content-part array. `ThreadMessageLike.content` is typed `readonly`
 * so we build with a plain array and cast at the assignment site.
 */
type ContentPart = ThreadMessageLike['content'] extends string
  ? never
  : NonNullable<ThreadMessageLike['content']>[number]

/**
 * Builds the `ThreadMessageLike[]` for a merged event stream.
 *
 * `pendingPermissions` and `permissionResolution` are passed in so
 * `PermissionRequested` events can be rendered with the correct approval state
 * (pending → approval.approved === undefined; granted/denied → boolean). The
 * renderer uses assistant-ui's ToolFallback.Approval slot, which reads
 * `approval` from the part.
 */
export function eventsToMessages(
  events: AppEvent[],
  pendingPermissions: PendingPermission[],
  permissionResolution: Map<string, 'granted' | 'denied'>,
): ThreadMessageLike[] {
  const out: ThreadMessageLike[] = []
  // Monotonic id counter so every message has a stable id for assistant-ui's
  // reconciler. Event ids from the backend are unique per session, but some
  // events (e.g. merged StreamUpdates) share the last id, so we suffix.
  let idCounter = 0
  const nextId = (event: AppEvent) =>
    `evt-${event.id ?? 'x'}-${idCounter++}`

  for (const ev of events) {
    switch (ev.type) {
      case 'PromptSubmitted': {
        const content: ContentPart[] = []
        if (ev.content) content.push({ type: 'text', text: ev.content })
        if (ev.attachments && ev.attachments.length > 0) {
          for (const att of ev.attachments) {
            content.push(attachmentToImagePart(att) as ContentPart)
          }
        }
        if (ev.injectedContext && ev.injectedContext.length > 0) {
          const ctxPart: ContextInjectionPart = {
            type: 'data-context-injection',
            context: ev.injectedContext,
          }
          content.push(ctxPart as unknown as ContentPart)
        }
        out.push({
          id: nextId(ev),
          role: 'user',
          content: content.length ? (content as ThreadMessageLike['content']) : '',
        })
        break
      }

      case 'ResponseStarted': {
        out.push({
          id: nextId(ev),
          role: 'system',
          content: ev.content ?? 'Agent is thinking…',
        })
        break
      }

      case 'ModelChanged': {
        out.push(systemRowMessage(ev, nextId(ev), 'Model switched'))
        break
      }

      case 'ConnectionRestarted': {
        out.push(systemRowMessage(ev, nextId(ev), 'Session restarted'))
        break
      }

      case 'SessionResumed': {
        out.push(systemRowMessage(ev, nextId(ev), 'Session restarted'))
        break
      }

      case 'SessionInterrupted':
      case 'SessionCancelled': {
        out.push(systemRowMessage(ev, nextId(ev), 'Stopped'))
        break
      }

      case 'AgentExited': {
        out.push(
          systemRowMessage(
            ev,
            nextId(ev),
            'unknown error',
            'Agent exited: ',
            'destructive',
          ),
        )
        break
      }

      case 'FileRevisionUpdated': {
        out.push(systemRowMessage(ev, nextId(ev), '', 'edited '))
        break
      }

      case 'FileChangedOnDisk': {
        out.push(
          systemRowMessage(
            ev,
            nextId(ev),
            'File changed on disk',
            undefined,
            'destructive',
          ),
        )
        break
      }

      case 'SessionCreated':
      case 'SessionClosed': {
        // Lifecycle events don't render in the chat panel (prior behavior).
        break
      }

      case 'StreamUpdate': {
        if (ev.thought) {
          if (!ev.content) break
          out.push({
            id: nextId(ev),
            role: 'assistant',
            content: [{ type: 'reasoning', text: ev.content }],
            status: ev.streaming ? { type: 'running' } : { type: 'complete', reason: 'stop' },
          })
          break
        }
        if (!ev.content && !ev.streaming) break
        const label = !ev.streaming ? stopReasonLabel(ev.stopReason) : null
        const text = ev.content ?? ''
        // Carry the stop-reason label as metadata so the text renderer can
        // append it below the final assistant message (matching prior UI).
        out.push({
          id: nextId(ev),
          role: 'assistant',
          content: [{ type: 'text', text }],
          status: ev.streaming ? { type: 'running' } : { type: 'complete', reason: 'stop' },
          metadata: label ? { custom: { stopLabel: label } } : undefined,
        })
        break
      }

      case 'ToolStarted': {
        out.push(toolCallMessage(ev, nextId(ev), { running: true }))
        break
      }

      case 'ToolCompleted': {
        const failed = ev.summary === 'failed'
        out.push(
          toolCallMessage(ev, nextId(ev), {
            running: false,
            failed,
            result: ev.content ?? ev.summary ?? '',
          }),
        )
        break
      }

      case 'ShellCommandStarted': {
        out.push(
          toolCallMessage(ev, nextId(ev), {
            running: true,
            isShell: true,
            output: ev.content,
          }),
        )
        break
      }

      case 'ShellCommandCompleted': {
        const failed = ev.exitCode !== 0
        out.push(
          toolCallMessage(ev, nextId(ev), {
            running: false,
            failed,
            isShell: true,
            output: ev.content || ev.summary,
            exitCode: ev.exitCode,
          }),
        )
        break
      }

      case 'PermissionRequested': {
        const requestId = ev.requestId ?? ''
        const pending = pendingPermissions.find((p) => p.id === requestId)
        const resolution = permissionResolution.get(requestId)
        out.push(
          toolCallMessage(ev, nextId(ev), {
            running: false,
            isPermission: true,
            approval: {
              id: requestId,
              approved: resolution === 'granted' ? true : resolution === 'denied' ? false : undefined,
              options: pending?.optionDetails?.map((o) => ({
                id: o.id,
                label: o.name,
                kind: o.kind.replace(/_/g, '-'),
              })),
            },
          }),
        )
        break
      }

      case 'PlanUpdated': {
        const planPart: PlanPart = {
          type: 'data-plan',
          content: ev.content ?? '',
        }
        out.push({
          id: nextId(ev),
          role: 'assistant',
          content: [planPart] as unknown as ThreadMessageLike['content'],
          status: { type: 'complete', reason: 'stop' },
        })
        break
      }

      // Folded by mergedEvents or handled elsewhere — skip.
      case 'ShellOutputStreamed':
      case 'FileWritten':
      case 'PermissionGranted':
      case 'PermissionDenied':
        break

      default:
        // Unknown event types are silently dropped (prior behavior returned null).
        break
    }
  }

  return out
}

/** Builds an assistant message carrying a `data-system-row` part, rendered as
 * a compact centered row. We use `role: 'assistant'` (not `'system'`) because
 * assistant-ui's `fromThreadMessageLike` validation requires system messages
 * to have exactly one `text` part — data parts are only allowed on assistant
 * messages. The `data-system-row` part is dispatched to `SystemRowMessage` by
 * `AssistantThread`'s `renderPart`, so the visual result is identical. */
function systemRowMessage(
  ev: AppEvent,
  id: string,
  fallback: string,
  prefix?: string,
  variant?: 'muted' | 'destructive',
): ThreadMessageLike {
  const part: SystemRowPart = {
    type: 'data-system-row',
    content: ev.content,
    fallback,
    prefix,
    variant,
  }
  return {
    id,
    role: 'assistant',
    content: [part] as unknown as ThreadMessageLike['content'],
  }
}

interface ToolCallOptions {
  running: boolean
  failed?: boolean
  result?: string
  isShell?: boolean
  isPermission?: boolean
  output?: string
  exitCode?: number
  approval?: {
    id: string
    approved?: boolean
    options?: { id: string; label: string; kind: string }[]
  }
}

/** Builds an assistant message with a single tool-call content part. */
function toolCallMessage(
  ev: AppEvent,
  id: string,
  opts: ToolCallOptions,
): ThreadMessageLike {
  const meta: ToolCallMetadata = {
    toolKind: ev.toolKind,
    target: ev.target,
    command: ev.command,
    summary: ev.summary,
    exitCode: opts.exitCode,
    isShell: opts.isShell,
  }
  const toolCallId = ev.toolCallId ?? ev.requestId ?? id
  const toolName = opts.isShell
    ? 'shell'
    : opts.isPermission
      ? 'permission'
      : ev.tool ?? ev.toolKind ?? 'tool'
  // For permission requests, args carry the command/target so the approval UI
  // can display them. For tools/shells, argsText carries the command.
  const args: Record<string, unknown> = {}
  if (ev.command) args.command = ev.command
  if (ev.target) args.target = ev.target
  if (ev.tool) args.tool = ev.tool
  if (ev.toolKind) args.toolKind = ev.toolKind
  if (ev.options) args.options = ev.options

  const part: Record<string, unknown> = {
    type: 'tool-call',
    toolCallId,
    toolName,
    args,
    argsText: ev.command ?? '',
    result: opts.result,
    isError: opts.failed,
    approval: opts.approval,
  }

  return {
    id,
    role: 'assistant',
    content: [part] as unknown as ThreadMessageLike['content'],
    status:
      opts.isPermission && opts.approval?.approved === undefined
        ? { type: 'requires-action', reason: 'tool-calls' }
        : opts.running
          ? { type: 'running' }
          : { type: 'complete', reason: 'stop' },
    metadata: { custom: { toolCall: meta } },
  }
}
