import { useState, type ReactNode } from 'react'
import { Wrench, ChevronRight } from 'lucide-react'
import { cva } from 'class-variance-authority'

interface ToolExecutionBlockProps {
  /** Optional kind-specific icon; defaults to a gear (Wrench). */
  icon?: ReactNode
  label: string
  target?: string
  /** e.g. "[running]", "[completed]", "exit 0". */
  status?: string
  failed?: boolean
  command?: string
  output?: string
  /** Defaults to false (closed) per spec. */
  defaultOpen?: boolean
}

const accordionSummary = cva(
  'flex items-center gap-1.5 cursor-pointer text-xs text-muted-foreground hover:text-foreground w-max select-none',
)

/**
 * Collapsible tool-execution trace item — borderless accordion, closed by
 * default. Summary shows a (kind-specific or gear) icon, label, optional
 * target, and status. Expanded body shows uppercase "COMMAND" / "OUTPUT"
 * labels above monospaced terminal text inside a hard-capped (~250px)
 * scrollable container. See agent_chat_update.md §2 (Tool Execution block).
 */
export function ToolExecutionBlock({
  icon,
  label,
  target,
  status,
  failed,
  command,
  output,
  defaultOpen,
}: ToolExecutionBlockProps) {
  const [open, setOpen] = useState(defaultOpen ?? false)
  return (
    <details
      className="group"
      open={open}
      onToggle={(e) => setOpen(e.currentTarget.open)}
    >
      <summary
        className={`${accordionSummary()} list-none [&::-webkit-details-marker]:hidden`}
      >
        {icon ?? <Wrench className="w-3.5 h-3.5" />}
        {label}
        {target && (
          <span className="text-muted-foreground/70 truncate max-w-[12rem]">
            {target}
          </span>
        )}
        {status && (
          <span className={failed ? 'text-destructive' : 'text-muted-foreground/70'}>
            {status}
          </span>
        )}
        <ChevronRight className="w-3 h-3 transition-transform group-open:rotate-90" />
      </summary>
      <div className="mt-1.5 ml-3 pl-3 border-l border-border bg-tool-call/60 p-2 max-h-[250px] overflow-y-auto">
        {command && (
          <>
            <div className="text-[10px] uppercase tracking-wide text-muted-foreground mb-1">
              Command
            </div>
            <pre className="font-mono text-xs text-muted-foreground whitespace-pre-wrap break-words mb-2">
              {command}
            </pre>
          </>
        )}
        {output && (
          <>
            <div
              className={`text-[10px] uppercase tracking-wide mb-1 ${
                failed ? 'text-destructive' : 'text-muted-foreground'
              }`}
            >
              {failed ? 'Error' : 'Output'}
            </div>
            <pre
              className={`font-mono text-xs whitespace-pre-wrap break-words ${
                failed ? 'text-destructive' : 'text-muted-foreground'
              }`}
            >
              {output}
            </pre>
          </>
        )}
        {/* No output but failed — surface a hint so the user isn't left
            guessing why the tool failed with no details. */}
        {failed && !output && (
          <pre className="font-mono text-xs text-destructive whitespace-pre-wrap break-words">
            Tool failed. The agent did not report error details. If this was a file read, the path may be outside the workspace.
          </pre>
        )}
      </div>
    </details>
  )
}
