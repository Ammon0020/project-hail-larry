import { AlertTriangle, Folder, Laptop } from 'lucide-react'
import type { Agent } from '../../types'
import { cn } from '@/lib/utils'
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/tooltip'

interface WorkspaceBarProps {
  /** Agents list for the harness selector. */
  agents: Agent[]
  /** Currently effective agent id (drives the harness label). */
  currentAgentId: string
  /** Harness change handler. */
  onSelectAgent: (id: string) => void
  /** Workspaces list. */
  workspaces: { id: string; name: string }[]
  /** Currently active workspace id. */
  workspaceId: string
  /** Workspace change handler. */
  onSelectWorkspace: (id: string) => void
  /** If true, the workspace switcher is disabled (e.g. because the session has started). */
  workspaceDisabled?: boolean
  /** Disabled state passthrough. */
  disabled?: boolean
}

/**
 * Bottom workspace bar — sits below the chat composer. Shows the execution
 * harness (agent) selector on the left and the active workspace switcher on the
 * right.
 *
 * See agent_chat_update.htm (workspace bar section).
 */
export function WorkspaceBar({
  agents,
  currentAgentId,
  onSelectAgent,
  workspaces,
  workspaceId,
  onSelectWorkspace,
  workspaceDisabled,
  disabled,
}: WorkspaceBarProps) {
  // Resolve the currently effective agent so we can render its display name.
  // Falls back to "Agent" when the id doesn't match (e.g. transient state).
  const currentAgent = agents.find((a) => a.id === currentAgentId)
  
  // Resolve the currently active workspace.
  const currentWorkspace = workspaces.find((w) => w.id === workspaceId)

  return (
    // pb-16 on mobile clears the fixed mobile bottom-nav; lg:pb-[3px] restores a
    // tight bottom inset on desktop where there is no bottom-nav. This padding
    // was moved here from ChatComposer's wrapper so the bar owns its own
    // clearance.
    <div className="flex shrink-0 items-center gap-4 px-3 pt-0 pb-16 text-xs text-muted-foreground lg:pt-0 lg:pb-[3px]">
      {/* Harness selector: an invisible native <select> overlays a styled
          label row so we get accessible option rendering for free while the
          visible chrome (icon + name) is fully custom. */}
      <div
        className="relative flex cursor-pointer items-center gap-1.5 rounded px-1 py-0.5 text-muted-foreground transition-colors hover:bg-white/[0.03] hover:text-foreground"
        title="Execution harness"
      >
        <Laptop className="size-3.5" strokeWidth={1.5} />
        {/* pointer-events-none so clicks fall through to the overlay select */}
        <span className="pointer-events-none">
          {currentAgent?.name ?? 'Agent'}
        </span>
        {currentAgent?.warning && (
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="pointer-events-auto relative z-10 flex items-center text-warning">
                <AlertTriangle className="size-3" />
              </span>
            </TooltipTrigger>
            <TooltipContent>{currentAgent.warning}</TooltipContent>
          </Tooltip>
        )}
        <select
          value={currentAgentId}
          onChange={(e) => onSelectAgent(e.target.value)}
          disabled={disabled}
          className="absolute inset-0 size-full cursor-pointer appearance-none bg-transparent opacity-0"
        >
          {agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.warning ? `(!) ${a.name}` : a.name}
            </option>
          ))}
        </select>
      </div>

      {/* Workspace switcher: same overlay pattern as the harness selector. */}
      <div 
        className={cn(
          "relative flex cursor-pointer items-center gap-1.5 rounded px-1 py-0.5 text-muted-foreground transition-colors",
          (disabled || workspaceDisabled) ? "cursor-not-allowed opacity-50" : "hover:bg-white/[0.03] hover:text-foreground"
        )}
        title={workspaceDisabled ? "Cannot change workspace mid-conversation" : "Workspace"}
      >
        <Folder className="size-3.5" strokeWidth={1.5} />
        <span className="pointer-events-none">
          {currentWorkspace?.name ?? 'No workspace'}
        </span>
        <select
          value={workspaceId}
          onChange={(e) => onSelectWorkspace(e.target.value)}
          disabled={disabled || workspaceDisabled || workspaces.length === 0}
          className="absolute inset-0 size-full cursor-pointer appearance-none bg-transparent opacity-0 disabled:cursor-not-allowed"
        >
          {workspaces.map((w) => (
            <option key={w.id} value={w.id}>
              {w.name}
            </option>
          ))}
        </select>
      </div>
    </div>
  )
}
