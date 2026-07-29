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
        pushOrMergeToolCallMessage(out, ev, nextId, { running: true })
        break
      }

      case 'ToolCompleted': {
        const failed = ev.summary === 'failed'
        pushOrMergeToolCallMessage(out, ev, nextId, {
          running: false,
          failed,
          result: ev.content ?? ev.summary ?? '',
        })
        break
      }

      case 'ShellCommandStarted': {
        pushOrMergeToolCallMessage(out, ev, nextId, {
          running: true,
          isShell: true,
          output: ev.content,
        })
        break
      }

      case 'ShellCommandCompleted': {
        const failed = ev.exitCode !== 0
        pushOrMergeToolCallMessage(out, ev, nextId, {
          running: false,
          failed,
          isShell: true,
          output: ev.content || ev.summary,
          exitCode: ev.exitCode,
        })
        break
      }

      case 'PermissionRequested': {
        const requestId = ev.requestId ?? ''
        const pending = pendingPermissions.find((p) => p.id === requestId)
        const resolution = permissionResolution.get(requestId)
        pushOrMergeToolCallMessage(out, ev, nextId, {
          running: false,
          isPermission: true,
          approval: {
            id: requestId,
            approved: resolution === 'granted' ? true : resolution === 'denied' ? false : undefined,
            options: approvalOptionsFor(pending),
          },
        })
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

/**
 * Backend decision kinds that persist a grant beyond the immediate tool call.
 * Only these get a confirm step + `grants` preview in the ToolFallback UI,
 * because clicking them writes a row the user can't see otherwise. One-shot
 * kinds (`allow_once`, `deny`) resolve and forget, so they need no preview.
 */
const DURABLE_DECISION_KINDS = new Set([
  'allow_session',
  'allow_always',
  'reject_always',
  'allow_tool_kind',
])

/**
 * Maps an ACP `PermissionOptionKind` string (as carried in
 * `optionDetails[].kind`) to the backend `PermissionDecision` value that
 * `POST /api/permissions/:id/respond` expects. ACP `reject_once` maps to
 * backend `deny` — the strings differ, so this table is required.
 * See `src/acp/core/handlers/permission.rs::permission_decision`.
 */
const ACP_KIND_TO_DECISION: Record<string, string> = {
  allow_once: 'allow_once',
  allow_always: 'allow_always',
  reject_once: 'deny',
  reject_always: 'reject_always',
}

/**
 * Maps a backend `PermissionDecision` value to the kebab-case `kind` string
 * that the vendored `ToolFallback` uses for label lookup via
 * `APPROVAL_OPTION_DEFAULT_LABELS`. Backend `deny` maps to `reject-once`
 * (the one-shot refusal label), not `deny` — the label map has no `deny` key.
 */
const DECISION_TO_TOOLFALLBACK_KIND: Record<string, string> = {
  allow_once: 'allow-once',
  allow_session: 'allow-session',
  allow_always: 'allow-always',
  deny: 'reject-once',
  reject_always: 'reject-always',
  allow_tool_kind: 'allow-tool-kind',
}

/**
 * Builds a human-readable description of exactly what a durable permission
 * decision would persist, shown in the ToolFallback confirm step before the
 * user commits. Format mirrors the grant shape the backend stores
 * (`PermissionDecision` + `PendingPermission` fields):
 *   - File-oriented (target set):  `edit_file` on `src/main.rs` — this session only
 *   - Shell-oriented (command set): `execute`: `npm test` — forever, all sessions
 *   - Fallback (tool only):         `tool_name` — forever, all sessions
 *
 * The scope suffix is derived from the decision kind: `allow_always` /
 * `reject_always` persist across sessions ("forever, all sessions"), while
 * `allow_session` is cleared when the session closes ("this session only").
 */
function grantDescriptionFor(
  pending: PendingPermission,
  decision: string,
): string {
  // `allow_tool_kind` ignores target/command — the grant covers ALL operations
  // of this tool type, not just this one. The description must NOT include the
  // specific target/command (it would be misleading to name one file when the
  // grant applies to every file).
  if (decision === 'allow_tool_kind') {
    return `\`${pending.tool}\` (any target) — forever, all sessions`
  }

  const scope =
    decision === 'allow_session' ? 'this session only' : 'forever, all sessions'

  if (pending.target) {
    // File-oriented permission: target is a workspace-relative path.
    return `\`${pending.tool}\` on \`${pending.target}\` — ${scope}`
  }
  if (pending.command) {
    // Shell-oriented permission: command is the literal argv the agent wants to run.
    return `\`${pending.tool}\`: \`${pending.command}\` — ${scope}`
  }
  // Fallback when neither target nor command is present (rare; e.g. a generic
  // tool grant with no path/argv scope). Still names the tool so the user has
  // *something* concrete to read before committing.
  return `\`${pending.tool}\` — ${scope}`
}

/**
 * Builds the ToolFallback-facing option list for a pending permission.
 *
 * The backend only populates `optionDetails` when the ACP agent supplied
 * explicit per-option labels; otherwise the field is omitted from JSON
 * (`skip_serializing_if` on empty), and we must fall back to the always-set
 * `options` array of raw decision ids (e.g. `allow_once`, `allow_session`,
 * `allow_always`, `deny`) that `PermissionManager::request` defaults to. When
 * falling back we leave `label` unset so `ToolFallback`'s default label map
 * (see `APPROVAL_OPTION_DEFAULT_LABELS` in the vendored tool-fallback.tsx)
 * supplies a human label instead of showing the raw decision string.
 *
 * `id` is always the backend's snake_case decision kind (`allow_once`,
 * `allow_session`, `allow_always`, `deny`, `reject_always`) rather than the
 * agent's opaque ACP option id: `POST /permissions/:id/respond` decodes
 * `decision` straight into `PermissionDecision`, and `acp/core/handlers/
 * permission.rs` re-derives the matching ACP option by kind, not by that id.
 * `AssistantThread` forwards `respondToApproval`'s `optionId` as `decision`
 * verbatim, so this must already be a valid `PermissionDecision` value.
 *
 * Durable decisions (`allow_session`, `allow_always`, `reject_always`) also
 * carry `confirm: true` and a `grants: [string]` array. The vendored
 * ToolFallback renders a confirm step listing each grant string before
 * resolving the option, so the user can see exactly what will be persisted
 * (tool + scope) before clicking "Confirm". One-shot kinds (`allow_once`,
 * `deny`) resolve immediately and persist nothing, so they omit both fields.
 */
function approvalOptionsFor(
  pending: PendingPermission | undefined,
): { id: string; label?: string; kind: string; confirm?: boolean; grants?: string[] }[] | undefined {
  if (!pending) return undefined

  // Build a unified list of { decision, label } pairs. When the ACP agent
  // supplied option details, map each ACP kind to the backend decision
  // (reject_once → deny). Otherwise fall back to the always-set `options`
  // array of raw decision ids and leave labels unset so ToolFallback's
  // default label map supplies human-readable text.
  const details = pending.optionDetails?.length
    ? pending.optionDetails.map((o) => ({
        decision: ACP_KIND_TO_DECISION[o.kind] ?? o.kind,
        label: o.name,
      }))
    : pending.options?.map((decision) => ({
        decision,
        label: undefined as string | undefined,
      }))

  if (!details?.length) return undefined

  return details.map(({ decision, label }) => {
    const tfKind = DECISION_TO_TOOLFALLBACK_KIND[decision] ?? decision.replace(/_/g, '-')
    const durable = DURABLE_DECISION_KINDS.has(decision)
    // For durable decisions, attach a confirm step + grant preview so the
    // user sees exactly what will be persisted before clicking "Confirm".
    // One-shot kinds resolve immediately and persist nothing, so they omit
    // both fields and skip the confirm step in the vendored ToolFallback.
    if (!durable) {
      return {
        id: decision,
        ...(label !== undefined && { label }),
        kind: tfKind,
      }
    }
    return {
      id: decision,
      ...(label !== undefined && { label }),
      kind: tfKind,
      confirm: true,
      grants: [grantDescriptionFor(pending, decision)],
    }
  })
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
    options?: { id: string; label?: string; kind: string; confirm?: boolean; grants?: string[] }[]
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
    result: opts.result ?? opts.output,
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

/**
 * Pushes a tool call message to `out`, or merges its tool-call part into the
 * preceding assistant message if that message is already a container for tool calls.
 * This groups consecutive tool call events in a turn into a single message
 * container, reducing DOM nodes and message reconciler overhead by up to 90%.
 */
function pushOrMergeToolCallMessage(
  out: ThreadMessageLike[],
  ev: AppEvent,
  nextId: (ev: AppEvent) => string,
  opts: ToolCallOptions,
) {
  const message = toolCallMessage(ev, nextId(ev), opts)
  const part = (message.content as ContentPart[])[0]
  const last = out[out.length - 1]

  if (
    last &&
    last.role === 'assistant' &&
    Array.isArray(last.content) &&
    last.content.length > 0 &&
    (last.content[last.content.length - 1] as Record<string, unknown>).type === 'tool-call'
  ) {
    ;(last.content as ContentPart[]).push(part)
    const PRECEDENCE = { 'requires-action': 2, 'running': 1, 'complete': 0 }
    const currentScore = PRECEDENCE[(last.status?.type as keyof typeof PRECEDENCE) ?? 'complete'] ?? 0
    const newScore = PRECEDENCE[(message.status?.type as keyof typeof PRECEDENCE) ?? 'complete'] ?? 0

    if (newScore > currentScore) {
      out[out.length - 1] = {
        ...last,
        status: message.status,
      }
    }
  } else {
    out.push(message)
  }
}

