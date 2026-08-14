import { ChevronRight, Layers } from 'lucide-react'
import type { InjectedContext } from '@/types'

interface ContextInjectionBlockProps {
  context: InjectedContext[]
}

/**
 * Collapsible, user-side trace of text/resources added by the daemon to a
 * prompt. It stays closed by default because open-file context can be large,
 * while retaining the exact text that was sent to the agent.
 */
export function ContextInjectionBlock({ context }: ContextInjectionBlockProps) {
  return (
    <details className="group mt-2 text-left" data-testid="prompt-context">
      <summary className="flex cursor-pointer list-none items-center gap-1.5 text-xs text-muted-foreground select-none hover:text-foreground [&::-webkit-details-marker]:hidden">
        <Layers className="size-3.5" />
        Context added ({context.length})
        <ChevronRight className="size-3 transition-transform group-open:rotate-90" />
      </summary>
      <div className="mt-1.5 max-h-[250px] overflow-y-auto border-l border-border bg-tool-call/60 p-2">
        {context.map((item, index) => (
          <section key={`${item.name}-${index}`} className={index > 0 ? 'mt-3' : undefined}>
            <h3 className="text-[10px] tracking-wide text-muted-foreground uppercase">
              {item.name}
            </h3>
            <pre className="mt-1 font-mono text-xs break-words whitespace-pre-wrap text-muted-foreground">
              {item.content}
            </pre>
          </section>
        ))}
      </div>
    </details>
  )
}
