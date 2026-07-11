import { Brain, ChevronRight } from 'lucide-react'
import { cva } from 'class-variance-authority'

interface ThinkingBlockProps {
  content: string
}

const accordionSummary = cva(
  'flex items-center gap-1.5 cursor-pointer text-xs text-muted-foreground hover:text-foreground w-max select-none',
)

/**
 * Collapsible "Thinking" trace item — borderless accordion, closed by default.
 * Summary is a small monochrome brain icon + label; expanded body uses a
 * left-border indent over the darker editor surface with italic muted text.
 * See agent_chat_update.md §2 (Chat History — Thinking block).
 */
export function ThinkingBlock({ content }: ThinkingBlockProps) {
  return (
    <details className="group">
      <summary
        className={`${accordionSummary()} list-none [&::-webkit-details-marker]:hidden`}
      >
        <Brain className="w-3.5 h-3.5" />
        Thinking
        <ChevronRight className="w-3 h-3 transition-transform group-open:rotate-90" />
      </summary>
      <div className="mt-1.5 ml-3 pl-3 border-l border-border bg-tool-call/60 p-2 italic text-xs text-muted-foreground whitespace-pre-wrap">
        {content}
      </div>
    </details>
  )
}
