import {
  Bot,
  Wrench,
  ChevronRight,
  ShieldAlert,
  Brain,
  ListChecks,
  Terminal,
  FilePen,
  FileSearch,
  Play,
  RefreshCw,
} from 'lucide-react'
import type { AppEvent } from '@/types'
import type { PendingPermission } from '@/lib/api'

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

/** Human label + button style for a permission option kind. */
function optionStyle(kind: string): string {
  if (kind.startsWith('reject') || kind === 'deny') {
    return 'bg-gray-700 hover:bg-gray-600'
  }
  return 'bg-blue-600 hover:bg-blue-500'
}

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
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-gray-800 flex items-center justify-center shrink-0 border border-gray-700 text-xs font-medium">
            U
          </div>
          <div className="flex-1 pt-0.5">
            <p className="text-sm text-gray-200">{event.content}</p>
          </div>
        </div>
      )

    case 'ResponseStarted':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <p className="text-sm text-gray-500 mb-2 animate-pulse">{event.content || 'Agent is thinking…'}</p>
          </div>
        </div>
      )

    case 'ToolStarted':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <details className="group mt-2 border-l-2 border-gray-800 pl-3 open:border-blue-500/50 transition-colors" open>
              <summary className="flex items-center gap-2 cursor-pointer text-xs font-mono text-gray-400 hover:text-gray-300 w-max select-none">
                <ChevronRight className="w-3.5 h-3.5 group-open:rotate-90 transition-transform" />
                {toolKindIcon(event.toolKind)}
                {event.tool || 'Tool'}
                {event.target && <span className="text-gray-300">{event.target}</span>}
                <span className="text-gray-600">[running]</span>
              </summary>
              {event.command && (
                <pre className="mt-2 bg-tool-call rounded-md border border-gray-800/80 p-2 text-xs whitespace-pre-wrap text-gray-400">{event.command}</pre>
              )}
            </details>
          </div>
        </div>
      )

    case 'ToolCompleted': {
      const failed = event.summary === 'failed'
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <details className="group mt-2 border-l-2 border-gray-800 pl-3 open:border-blue-500/50 transition-colors">
              <summary className="flex items-center gap-2 cursor-pointer text-xs font-mono text-gray-400 hover:text-gray-300 w-max select-none">
                <ChevronRight className="w-3.5 h-3.5 group-open:rotate-90 transition-transform" />
                {toolKindIcon(event.toolKind)}
                {event.toolKind || 'tool'}
                {event.target && <span className="text-gray-300">{event.target}</span>}
                <span className={failed ? 'text-red-400' : 'text-gray-600'}>[{event.summary || 'completed'}]</span>
              </summary>
              {event.content && (
                <pre className="mt-2 bg-tool-call rounded-md border border-gray-800/80 p-2 text-xs whitespace-pre-wrap text-gray-400 overflow-x-auto">{event.content}</pre>
              )}
            </details>
          </div>
        </div>
      )
    }

    case 'PlanUpdated':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <div className="bg-tool-call border border-gray-800 rounded-lg p-2.5 text-xs">
              <div className="text-gray-400 font-semibold flex items-center gap-1.5 mb-1.5">
                <ListChecks className="w-3.5 h-3.5" /> Plan
              </div>
              <pre className="whitespace-pre-wrap text-gray-300 font-sans">{event.content}</pre>
            </div>
          </div>
        </div>
      )

    case 'ShellCommandStarted':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
            <Terminal className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <div className="bg-black/60 border border-gray-800 rounded-lg px-2.5 py-1.5 text-xs font-mono text-gray-400">
              $ {event.command}
            </div>
          </div>
        </div>
      )

    case 'ShellCommandCompleted':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
            <Terminal className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <div className="bg-black/60 border border-gray-800 rounded-lg overflow-hidden text-xs font-mono">
              <div className="px-2.5 py-1.5 bg-gray-900/80 text-gray-400 flex items-center justify-between">
                <span>$ {event.command}</span>
                <span className={event.exitCode === 0 ? 'text-green-400' : 'text-red-400'}>exit {event.exitCode ?? '?'}</span>
              </div>
              {event.summary && (
                <pre className="p-2.5 text-gray-300 whitespace-pre-wrap">{event.summary}</pre>
              )}
            </div>
          </div>
        </div>
      )

    case 'ShellOutputStreamed':
      return null

    case 'PermissionRequested': {
      const requestId = event.requestId || ''
      // Resolved (answered here or on another device): show the outcome.
      if (resolution) {
        return (
          <div className="flex gap-3">
            <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
              <ShieldAlert className="w-4 h-4" />
            </div>
            <p className="text-xs text-gray-500 pt-1.5">
              Permission {resolution === 'denied' ? 'denied' : 'granted'} — {event.tool}
            </p>
          </div>
        )
      }
      // Still pending: render one button per backend-provided option.
      const options = pending?.optionDetails ?? (event.options ?? []).map((id) => ({ id, name: id, kind: id }))
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <div className="mt-2 bg-blue-900/10 border border-blue-500/30 rounded-lg p-2.5">
              <div className="flex items-center gap-2 text-xs text-blue-400 font-semibold mb-2">
                <ShieldAlert className="w-3.5 h-3.5" /> Permission Required
              </div>
              <p className="text-xs text-gray-300 mb-2.5">
                <span className="font-medium">{event.tool}</span>
                {event.target && <span className="text-gray-400"> · {event.target}</span>}
              </p>
              {event.command && (
                <pre className="text-xs font-mono text-gray-400 bg-black/40 px-1.5 py-1 rounded mb-2.5 whitespace-pre-wrap">{event.command}</pre>
              )}
              {!pending && (
                <p className="text-[11px] text-gray-500 mb-2">This request is no longer active.</p>
              )}
              <div className="grid grid-cols-2 gap-2">
                {options.map((o) => (
                  <button
                    key={o.id}
                    disabled={!pending}
                    onClick={() => onPermissionResponse?.(requestId, o.id)}
                    className={`text-white text-xs font-medium py-1.5 rounded transition disabled:opacity-50 disabled:cursor-not-allowed ${optionStyle(o.kind)}`}
                  >
                    {o.name}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>
      )
    }

    case 'StreamUpdate':
      // Agent thoughts render as a muted, collapsible block.
      if (event.thought) {
        if (!event.content) return null
        return (
          <div className="flex gap-3">
            <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
              <Brain className="w-4 h-4" />
            </div>
            <details className="flex-1 pt-0.5">
              <summary className="text-xs text-gray-500 italic cursor-pointer">Thinking…</summary>
              <p className="whitespace-pre-wrap text-xs text-gray-500 mt-1 pl-1 border-l border-gray-800">{event.content}</p>
            </details>
          </div>
        )
      }
      if (!event.content && !event.streaming) return null
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <p className="whitespace-pre-wrap text-sm text-gray-300">
              {event.content}
              {event.streaming && (
                <span className="inline-block w-1.5 h-4 ml-0.5 bg-blue-400 animate-pulse align-text-bottom" />
              )}
            </p>
          </div>
        </div>
      )

    case 'AgentExited':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-red-600/20 text-red-400 flex items-center justify-center shrink-0 border border-red-500/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <p className="text-sm text-red-300">Agent exited: {event.summary || event.content || 'unknown error'}</p>
          </div>
        </div>
      )

    case 'ConnectionRestarted':
    case 'SessionResumed':
      return (
        <div className="flex justify-center">
          <span className="text-[11px] text-gray-500 bg-gray-800/60 rounded-full px-3 py-1 inline-flex items-center gap-1.5">
            <RefreshCw className="w-3 h-3" /> {event.content || 'Session restarted'}
          </span>
        </div>
      )

    case 'SessionCancelled':
    case 'SessionInterrupted':
      return (
        <div className="flex justify-center">
          <span className="text-[11px] text-gray-500 bg-gray-800/60 rounded-full px-3 py-1">Stopped</span>
        </div>
      )

    case 'FileRevisionUpdated':
      return (
        <div className="flex justify-center">
          <span className="text-[11px] text-gray-500">edited {event.content}</span>
        </div>
      )

    case 'PermissionGranted':
    case 'PermissionDenied':
      return null

    default:
      return null
  }
}
