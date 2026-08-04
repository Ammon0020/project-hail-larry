/**
 * Custom data-part and image renderers for assistant-ui's
 * `MessagePrimitive.Parts`.
 *
 * Text, reasoning, and tool-call parts are now rendered by the styled
 * assistant-ui registry components (`MarkdownText`, `Reasoning`,
 * `ToolFallback`) directly from `AssistantThread`. This file retains only the
 * renderers for parts that have no styled registry equivalent: image
 * attachments, system rows, plan updates, and context-injection blocks.
 */
import type { ImageMessagePartProps } from '@assistant-ui/react'
import { ListChecks } from 'lucide-react'

import { SystemRow } from './SystemRow'
import { ContextInjectionBlock } from './ContextInjectionBlock'

/** Renders image attachments in user messages as small thumbnails. */
export function AssistantImagePart({ image, filename }: ImageMessagePartProps) {
  return (
    <div className="flex flex-wrap gap-2 mt-2">
      <a
        href={image}
        target="_blank"
        rel="noopener noreferrer"
        className="group block rounded-lg border border-border bg-muted p-1.5 hover:border-ring transition"
        title={filename}
      >
        <img
          src={image}
          alt={filename ?? ''}
          className="w-20 h-20 rounded-md object-cover border border-border"
        />
        <div className="mt-1 text-[11px] text-muted-foreground truncate max-w-[80px]">
          {filename}
        </div>
      </a>
    </div>
  )
}

/** Renders a `data-system-row` part via the existing SystemRow component. */
export function SystemRowMessage({
  content,
  fallback,
  prefix,
  variant,
}: {
  content?: string
  fallback: string
  prefix?: string
  variant?: 'muted' | 'destructive'
}) {
  return <SystemRow content={content} fallback={fallback} prefix={prefix} variant={variant} />
}

/** Renders a `data-context-injection` part via the existing ContextInjectionBlock. */
export function ContextInjectionPartView({
  context,
}: {
  context: { name: string; content: string }[]
}) {
  return <ContextInjectionBlock context={context} />
}

/** Renders a `data-plan` part as a compact ListChecks row. */
export function PlanPartView({ content }: { content: string }) {
  return (
    <div className="flex justify-start">
      <div className="text-xs text-muted-foreground flex items-center gap-1.5">
        <ListChecks className="w-3.5 h-3.5 shrink-0" />
        <span className="text-foreground whitespace-pre-wrap">{content}</span>
      </div>
    </div>
  )
}
