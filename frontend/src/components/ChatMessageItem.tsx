import { Bot, Wrench, ChevronRight, ShieldAlert } from 'lucide-react'
import type { AppEvent } from '@/types'

/**
 * Renders a single event from the event stream as a chat message,
 * tool timeline card, or permission dialog (Blueprint Sec 11).
 *
 * In production, events arrive over WebSocket as JSON and are rendered
 * chronologically. The UI is derived from the event stream.
 */
export function ChatMessageItem({
  event,
  onPermissionResponse,
}: {
  event: AppEvent
  onPermissionResponse?: (sessionId: string, decision: 'allow' | 'deny') => void
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
            <p className="text-sm text-gray-300 mb-2">{event.content}</p>
          </div>
        </div>
      )

    case 'ToolCompleted':
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <details className="group mt-2 border-l-2 border-gray-800 pl-3 open:border-blue-500/50 transition-colors">
              <summary className="flex items-center gap-2 cursor-pointer text-xs font-mono text-gray-400 hover:text-gray-300 w-max select-none">
                <ChevronRight className="w-3.5 h-3.5 group-open:rotate-90 transition-transform" />
                <Wrench className="w-3.5 h-3.5 text-purple-400" />
                {event.tool}: {event.target} <span className="text-gray-600">[completed]</span>
              </summary>
              <div className="mt-2 space-y-1.5 pl-1">
                <div className="bg-tool-call rounded-md border border-gray-800/80 p-2 text-xs">
                  <span className="text-purple-400 font-mono">tool_call</span>: {event.summary}
                </div>
              </div>
            </details>
          </div>
        </div>
      )

    case 'PermissionRequested':
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
                <span className="font-mono text-gray-400 bg-black/40 px-1 rounded">
                  {event.tool}: {event.command}
                </span>
              </p>
              <div className="flex gap-2">
                <button
                  onClick={() => onPermissionResponse?.(event.sessionId, 'allow')}
                  className="flex-1 bg-blue-600 hover:bg-blue-500 text-white text-xs font-medium py-1.5 rounded transition"
                >
                  Allow
                </button>
                <button
                  onClick={() => onPermissionResponse?.(event.sessionId, 'deny')}
                  className="flex-1 bg-gray-700 hover:bg-gray-600 text-white text-xs font-medium py-1.5 rounded transition"
                >
                  Deny
                </button>
              </div>
            </div>
          </div>
        </div>
      )

    case 'StreamUpdate':
      if (event.streaming) {
        return (
          <div className="flex gap-3">
            <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
              <Bot className="w-4 h-4" />
            </div>
            <div className="flex-1 pt-1">
              <div className="flex items-center gap-1.5 text-xs text-gray-500">
                <div className="flex gap-1">
                  <div className="w-1.5 h-1.5 rounded-full bg-blue-400 animate-bounce" style={{ animationDelay: '0ms' }} />
                  <div className="w-1.5 h-1.5 rounded-full bg-blue-400 animate-bounce" style={{ animationDelay: '150ms' }} />
                  <div className="w-1.5 h-1.5 rounded-full bg-blue-400 animate-bounce" style={{ animationDelay: '300ms' }} />
                </div>
                {event.content}
              </div>
            </div>
          </div>
        )
      }
      return (
        <div className="flex gap-3">
          <div className="w-7 h-7 rounded-lg bg-blue-600/20 text-blue-400 flex items-center justify-center shrink-0 border border-blue-500/30">
            <Bot className="w-4 h-4" />
          </div>
          <div className="flex-1 pt-0.5">
            <p className="text-sm text-gray-300">{event.content}</p>
          </div>
        </div>
      )

    case 'PermissionGranted':
    case 'PermissionDenied':
      // These events don't render as separate messages —
      // in production, the permission dialog updates its state
      return null

    default:
      return null
  }
}
