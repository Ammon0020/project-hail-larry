import { Folder, Laptop } from 'lucide-react'
import type { Agent } from '../../types'

interface WorkspaceBarProps {
  /** Agents list for the harness selector. */
  agents: Agent[]
  /** Currently effective agent id (drives the harness label). */
  currentAgentId: string
  /** Harness change handler. */
  onSelectAgent: (id: string) => void
  /** Static workspace name shown with the folder icon (non-interactive). */
  workspaceName: string
  /** Disabled state passthrough. */
  disabled?: boolean
}

/**
 * Bottom workspace bar — sits below the chat composer. Shows the execution
 * harness (agent) selector on the left and the active workspace name on the
 * right. The workspace name is non-interactive (display only).
 *
 * See agent_chat_update.htm (workspace bar section).
 */
export function WorkspaceBar({
  agents,
  currentAgentId,
  onSelectAgent,
  workspaceName,
  disabled,
}: WorkspaceBarProps) {
  // Resolve the currently effective agent so we can render its display name.
  // Falls back to "Agent" when the id doesn't match (e.g. transient state).
  const currentAgent = agents.find((a) => a.id === currentAgentId)

  return (
    // pb-20 on mobile clears the fixed mobile bottom-nav; lg:pb-3 restores a
    // tight bottom inset on desktop where there is no bottom-nav. This padding
    // was moved here from ChatComposer's wrapper so the bar owns its own
    // clearance.
    <div className="flex items-center gap-4 px-3 pt-2.5 pb-20 lg:pb-3 text-xs text-muted-foreground shrink-0">
      {/* Harness selector: an invisible native <select> overlays a styled
          label row so we get accessible option rendering for free while the
          visible chrome (icon + name) is fully custom. */}
      <div
        className="relative flex items-center gap-1.5 px-1 py-0.5 rounded text-muted-foreground hover:text-foreground hover:bg-white/[0.03] transition-colors cursor-pointer"
        title="Execution harness"
      >
        <Laptop className="w-3.5 h-3.5" strokeWidth={1.5} />
        {/* pointer-events-none so clicks fall through to the overlay select */}
        <span className="pointer-events-none">
          {currentAgent?.name ?? 'Agent'}
        </span>
        <select
          value={currentAgentId}
          onChange={(e) => onSelectAgent(e.target.value)}
          disabled={disabled}
          className="absolute inset-0 w-full h-full opacity-0 cursor-pointer appearance-none bg-transparent"
        >
          {agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      </div>

      {/* Workspace name: display only. pointer-events-none keeps it out of
           the hit-test so it never steals clicks from siblings. */}
      <div className="flex items-center gap-1.5 px-1 py-0.5 pointer-events-none">
        <Folder className="w-3.5 h-3.5" strokeWidth={1.5} />
        <span>{workspaceName || 'No workspace'}</span>
      </div>
    </div>
  )
}
