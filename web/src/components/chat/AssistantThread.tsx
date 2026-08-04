import { useMemo } from 'react'
import type { ReactNode } from 'react'
import { ChevronDown, X } from 'lucide-react'
import {
  AssistantRuntimeProvider,
  MessagePrimitive,
  ThreadPrimitive,
  useExternalStoreRuntime,
} from '@assistant-ui/react'
import type {
  EnrichedPartState,
  ImageMessagePartProps,
  ReasoningMessagePartProps,
  ToolCallMessagePartProps,
} from '@assistant-ui/react'
import type { AppEvent } from '@/types'
import type { PendingPermission } from '@/lib/api'
import { eventsToMessages } from '@/lib/chatConverter'
import { MarkdownText } from '@/components/assistant-ui/markdown-text'
import { Reasoning } from '@/components/assistant-ui/reasoning'
import { ToolFallback } from '@/components/assistant-ui/tool-fallback'
import { Banner } from '../ui/Banner'
import {
  AssistantImagePart,
  ContextInjectionPartView,
  PlanPartView,
  SystemRowMessage,
} from './AssistantParts'

interface AssistantThreadProps {
  /** Already-merged events (consecutive StreamUpdates collapsed by ChatPanel). */
  events: AppEvent[]
  pendingPermissions: PendingPermission[]
  permissionResolution: Map<string, 'granted' | 'denied'>
  onPermissionResponse: (id: string, decision: string) => void
  /** True when the agent is actively producing events for this session. */
  isRunning: boolean
  error: string | null
  /** Whether another older event page can be loaded. */
  hasOlderEvents: boolean
  loadingOlderEvents: boolean
  onLoadOlder: () => void
  /** When true, shows a "MCP config changed — restart to apply" banner. */
  mcpConfigChanged?: boolean
  onDismissMcpBanner?: () => void
  onRestartForMcp?: () => void
  /** Placeholder for future wiring; ChatPanel owns the composer. */
  onNewMessage?: () => void
}

type CustomPartFields = {
  name?: string
  data?: {
    context?: { name: string; content: string }[]
    content?: string
    fallback?: string
    prefix?: string
    variant?: 'muted' | 'destructive'
  }
  context?: { name: string; content: string }[]
  content?: string
  fallback?: string
  prefix?: string
  variant?: 'muted' | 'destructive'
}

export function AssistantThread({
  events,
  pendingPermissions,
  permissionResolution,
  onPermissionResponse,
  isRunning,
  error,
  hasOlderEvents,
  loadingOlderEvents,
  onLoadOlder,
  mcpConfigChanged,
  onDismissMcpBanner,
  onRestartForMcp,
}: AssistantThreadProps) {
  const messages = useMemo(
    () => eventsToMessages(events, pendingPermissions, permissionResolution),
    [events, pendingPermissions, permissionResolution],
  )

  const runtime = useExternalStoreRuntime<ReturnType<typeof eventsToMessages>[number]>({
    messages,
    isRunning,
    convertMessage: (message) => message,
    onNew: async () => {},
    onCancel: async () => {},
    // `optionId` carries the backend's decision kind verbatim when the user
    // picked a declared option (allow_once/allow_session/allow_always/deny/
    // reject_always — see `approvalOptionsFor` in chatConverter.ts). The
    // plain Allow/Deny fallback (no declared options) has no optionId, so we
    // map the boolean to the two decisions every PermissionRequest accepts.
    onRespondToToolApproval: async ({ approvalId, approved, optionId }) => {
      onPermissionResponse(approvalId, optionId ?? (approved ? 'allow_once' : 'deny'))
    },
  })

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <ThreadPrimitive.Root className="relative flex min-h-0 flex-1 flex-col">
        <ThreadPrimitive.Viewport
          autoScroll
          className="h-full overflow-y-auto p-3 pb-20 lg:p-4 lg:pb-4"
        >
          {events.length === 0 && !error && (
            <div className="rounded-lg border border-border bg-panel/50 p-3 text-xs text-muted-foreground">
              Send a message to start a conversation.
            </div>
          )}

          {hasOlderEvents && (
            <div className="mb-3 flex justify-center">
              <button
                type="button"
                onClick={onLoadOlder}
                disabled={loadingOlderEvents}
                className="rounded border border-border px-3 py-1 text-xs text-muted-foreground hover:bg-accent disabled:cursor-wait disabled:opacity-60"
              >
                {loadingOlderEvents ? 'Loading older messages…' : 'Load older messages'}
              </button>
            </div>
          )}

          <ThreadPrimitive.Messages>
            {({ message }) => {
              if (message.role === 'user') return <UserMessage />
              if (message.role === 'system') return <SystemMessage />
              return <AssistantMessage />
            }}
          </ThreadPrimitive.Messages>

          {mcpConfigChanged && (
            <Banner
              variant="success"
              className="flex items-center justify-between gap-2 rounded-lg border p-3"
            >
              <span>MCP config changed — restart to apply</span>
              <div className="flex items-center gap-2">
                {onRestartForMcp && (
                  <button
                    onClick={onRestartForMcp}
                    className="rounded bg-primary px-2 py-0.5 font-medium text-primary-foreground transition hover:bg-primary/90"
                  >
                    Restart
                  </button>
                )}
                {onDismissMcpBanner && (
                  <button
                    onClick={onDismissMcpBanner}
                    className="text-primary/70 transition hover:text-primary"
                    aria-label="Dismiss"
                  >
                    <X className="h-3 w-3" />
                  </button>
                )}
              </div>
            </Banner>
          )}

          {error && (
            <Banner variant="error" className="rounded-lg border p-3">
              {error}
            </Banner>
          )}
        </ThreadPrimitive.Viewport>

        <ThreadPrimitive.ScrollToBottom
          className="absolute right-4 bottom-4 rounded-full border border-border bg-background p-2 text-muted-foreground shadow-md transition hover:bg-accent hover:text-foreground"
          title="Jump to bottom"
          aria-label="Jump to bottom"
        >
          <ChevronDown className="h-4 w-4" />
        </ThreadPrimitive.ScrollToBottom>
      </ThreadPrimitive.Root>
    </AssistantRuntimeProvider>
  )
}

function renderPart(part: EnrichedPartState): ReactNode {
  const customPart = part as unknown as CustomPartFields

  switch (part.type) {
    case 'text':
      return <MarkdownText />
    case 'reasoning':
      return <Reasoning {...(part as unknown as ReasoningMessagePartProps)} />
    case 'image':
      return <AssistantImagePart {...(part as unknown as ImageMessagePartProps)} />
    case 'tool-call':
      return <ToolFallback {...(part as unknown as ToolCallMessagePartProps)} />
    case 'data':
      return renderDataPart(customPart)
    default:
      return renderUnnormalizedDataPart(part, customPart)
  }
}

function renderDataPart(part: CustomPartFields): ReactNode {
  switch (part.name) {
    case 'context-injection':
      return <ContextInjectionPartView context={part.data?.context ?? []} />
    case 'plan':
      return <PlanPartView content={part.data?.content ?? ''} />
    case 'system-row':
      return (
        <SystemRowMessage
          content={part.data?.content}
          fallback={part.data?.fallback ?? ''}
          prefix={part.data?.prefix}
          variant={part.data?.variant}
        />
      )
    default:
      return null
  }
}

function renderUnnormalizedDataPart(
  part: EnrichedPartState,
  customPart: CustomPartFields,
): ReactNode {
  switch (part.type as string) {
    case 'data-context-injection':
      return <ContextInjectionPartView context={customPart.context ?? []} />
    case 'data-plan':
      return <PlanPartView content={customPart.content ?? ''} />
    case 'data-system-row':
      return (
        <SystemRowMessage
          content={customPart.content}
          fallback={customPart.fallback ?? ''}
          prefix={customPart.prefix}
          variant={customPart.variant}
        />
      )
    default:
      return null
  }
}

function UserMessage() {
  return (
    <div className="mb-3 flex justify-end">
      <div className="max-w-[85%] break-words rounded-[18px] rounded-br-[4px] bg-secondary px-3 py-2 text-sm text-foreground">
        <MessagePrimitive.Parts>{({ part }) => renderPart(part)}</MessagePrimitive.Parts>
      </div>
    </div>
  )
}

function AssistantMessage() {
  return (
    <div className="mb-3 space-y-1">
      <MessagePrimitive.Parts>{({ part }) => renderPart(part)}</MessagePrimitive.Parts>
    </div>
  )
}

function SystemMessage() {
  return (
    <div className="mb-3">
      <MessagePrimitive.Parts>{({ part }) => renderSystemPart(part)}</MessagePrimitive.Parts>
    </div>
  )
}

function renderSystemPart(part: EnrichedPartState): ReactNode {
  if (part.type === 'text') {
    const textPart = part as unknown as { text?: string }
    return <SystemRowMessage content={textPart.text} fallback="Agent is thinking…" />
  }

  return renderPart(part)
}
