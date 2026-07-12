import { type KeyboardEvent, useState, useRef, useEffect, useCallback } from 'react'
import { Plus, ArrowUp, Square, X, Loader2, Wrench, Code } from 'lucide-react'
import { cn } from '@/lib/utils'
import { McpPopout } from './chat/McpPopout'

interface ChatComposerProps {
  /** Available models for the current agent (derived in ChatPanel from currentAgent?.models). */
  models: { id: string; name: string }[]
  effectiveModelId: string
  onModelChange: (id: string) => void
  input: string
  onInputChange: (v: string) => void
  onSend: () => void
  onStop: () => void
  agentRunning: boolean
  canSend: boolean
  pendingPreviews: { url: string; name: string }[]
  onRemoveAttachment: (i: number) => void
  onPickFiles: () => void
  uploading: boolean
  uploadError: string | null
  disabled?: boolean
  /** MCP servers — moved here from ChatTabBar. */
  mcpServers: { name: string; enabled: boolean }[]
  /**
   * Optional health status keyed by server name (from `GET /api/mcp/status`).
   * Forwarded to McpPopout to drive status dot colors; absent entries fall
   * back to enabled-based coloring.
   */
  mcpStatusByName?: Record<string, { status: 'healthy' | 'unhealthy' | 'disabled' | 'unknown'; error?: string }>
  /** True while a health refresh is in flight — shows a spinner in the popout header. */
  mcpStatusLoading?: boolean
  onToggleMcpServer: (name: string, enabled: boolean) => void
  mcpTogglingServer: string | null
  /**
   * Fired when the user opens the MCP popout (false → true transition) so
   * ChatPanel can refresh health status on demand. Not fired on close.
   */
  onMcpPopoutOpen?: () => void
  /** Active profile mode (Code / Ask / Plan). */
  profile: 'Code' | 'Ask' | 'Plan'
  /** Callback when the user changes the profile mode. */
  onProfileChange: (profile: 'Code' | 'Ask' | 'Plan') => void
}

/**
 * Chat input composer — textarea + attach + Tools (MCP) + profile/model
 * hitbox selectors + send/stop, grouped into one rounded card (design §3).
 * The agent selector moved OUT to WorkspaceBar; MCP controls moved IN here.
 *
 * Selectors use the "hitbox" pattern: a styled label div overlays a fully
 * transparent native <select>, so we get native dropdown behavior without
 * the shadcn Select pill styling. The Tools button toggles an McpPopout
 * anchored above the button group.
 */
export function ChatComposer({
  models,
  effectiveModelId,
  onModelChange,
  input,
  onInputChange,
  onSend,
  onStop,
  agentRunning,
  canSend,
  pendingPreviews,
  onRemoveAttachment,
  onPickFiles,
  uploading,
  uploadError,
  disabled,
  mcpServers,
  mcpStatusByName,
  mcpStatusLoading,
  onToggleMcpServer,
  mcpTogglingServer,
  onMcpPopoutOpen,
  profile,
  onProfileChange,
}: ChatComposerProps) {
  // Tools popout visibility — toggled by the Wrench button, closed by
  // outside-click/Escape inside McpPopout.
  const [showMcpPopout, setShowMcpPopout] = useState(false)

  // Stable close handler for McpPopout (#11). Keeping the identity stable
  // avoids re-subscribing the outside-click/Escape listeners on every render
  // of ChatComposer (which would otherwise happen with an inline arrow).
  const handleMcpPopoutClose = useCallback(() => {
    setShowMcpPopout(false)
  }, [])

  const currentModel = models.find((m) => m.id === effectiveModelId)

  const textareaRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto'
      textareaRef.current.style.height = Math.min(textareaRef.current.scrollHeight, 120) + 'px'
    }
  }, [input])

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      onSend()
    }
  }

  return (
    // Outer wrapper — no mobile bottom padding here anymore; that moved to
    // WorkspaceBar so the bottom-nav layout can own its own spacing.
    <div className="pt-2.5 px-2.5 pb-0 lg:pt-3 lg:px-3 lg:pb-0 shrink-0 border-t border-border/50">
      {/* Pending attachment previews — above the card so the card stays clean. */}
      {pendingPreviews.length > 0 && (
        <div className="flex flex-wrap gap-2 mb-2">
          {pendingPreviews.map((preview, i) => (
            <div
              key={`${preview.url}-${i}`}
              className="relative group flex items-center gap-2 rounded-lg border border-border bg-muted px-2 py-1.5 pr-7 max-w-[180px]"
            >
              <img
                src={preview.url}
                alt={preview.name}
                className="w-8 h-8 rounded object-cover shrink-0 border border-border"
              />
              <span className="text-xs text-muted-foreground truncate" title={preview.name}>
                {preview.name}
              </span>
              <button
                onClick={() => onRemoveAttachment(i)}
                className="absolute right-1 top-1/2 -translate-y-1/2 p-0.5 rounded text-muted-foreground hover:text-foreground hover:bg-accent transition"
                title="Remove attachment"
                aria-label={`Remove ${preview.name}`}
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Upload error banner — above the card. */}
      {uploadError && (
        <div className="mb-2 rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive">
          {uploadError}
        </div>
      )}

      {/* Composer card — textarea on top, actions row below. Focus reveals a
          subtle border (transparent → border). No divider; gap-3 spaces them. */}
      <div className="bg-input rounded-xl px-3 pt-2 pb-1 flex flex-col gap-1.5 border border-transparent focus-within:border-border transition-colors">
        <textarea
          ref={textareaRef}
          value={input}
          onChange={(e) => onInputChange(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Type @ to bring in another conversation"
          disabled={disabled}
          rows={1}
          className="w-full bg-transparent border-0 outline-none text-[13px] text-foreground placeholder:text-muted-foreground resize-none py-1 disabled:opacity-60 disabled:cursor-not-allowed"
        />

        {/* Actions row: [attach|Tools group] [profile] [model] ... [send/stop] */}
        <div className="flex items-center justify-between">
          {/* Left controls. */}
          <div className="flex items-center gap-px">
            {/* Button group: Plus (attach) + Tools (MCP toggle). The McpPopout
                is anchored to this relative container. */}
            <div className="relative flex gap-px mr-1">
              {/* Plus (attach) — translucent icon button. */}
              <button
                onClick={onPickFiles}
                disabled={uploading || disabled}
                className="w-7 h-7 rounded-md flex items-center justify-center text-muted-foreground bg-white/[0.04] hover:bg-white/[0.12] hover:text-foreground transition disabled:opacity-60 disabled:cursor-not-allowed"
                title="Attach files"
                aria-label="Attach files"
              >
                {uploading ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <Plus className="w-3.5 h-3.5" strokeWidth={2.5} />
                )}
              </button>

              {/* Tools (MCP toggle) — same translucent styling; active state
                  lifts the background to match the popout-open cue.
                  `data-mcp-toggle` marks this button so McpPopout's
                  outside-click handler can ignore mousedowns on it (#1) —
                  otherwise the mousedown closes the popout and the
                  subsequent click reopens it, making the button unable to
                  close the popout. The onClick only fires onMcpPopoutOpen
                  on the false→true (opening) transition. */}
              <button
                data-mcp-toggle
                onClick={() =>
                  setShowMcpPopout((v) => {
                    if (!v) onMcpPopoutOpen?.()
                    return !v
                  })
                }
                className={cn(
                  'w-7 h-7 rounded-md flex items-center justify-center text-muted-foreground bg-white/[0.04] hover:bg-white/[0.12] hover:text-foreground transition',
                  showMcpPopout && 'bg-white/[0.12] text-foreground',
                )}
                title="Select MCP tools"
                aria-label="MCP tools"
                aria-expanded={showMcpPopout}
              >
                <Wrench className="w-3.5 h-3.5" strokeWidth={2.5} />
              </button>

              {/* MCP popout — absolute-positioned bottom-full left-0 by the
                  popout itself; rendered inside this relative group. */}
              {showMcpPopout && (
                <McpPopout
                  mcpServers={mcpServers}
                  statusByName={mcpStatusByName}
                  statusLoading={mcpStatusLoading}
                  onToggle={onToggleMcpServer}
                  togglingServer={mcpTogglingServer}
                  onClose={handleMcpPopoutClose}
                />
              )}
            </div>

            {/* Profile selector hitbox — local UI placeholder (v1). Native
                <select> overlaid transparently on a styled label. The label
                text is hidden on narrow screens (icon-only). */}
            <div
              className="relative flex items-center gap-1.5 px-2 py-1.5 rounded-md cursor-pointer text-muted-foreground hover:bg-white/[0.04] hover:text-foreground transition-colors"
              title="Profile context"
            >
              <Code className="w-3.5 h-3.5" strokeWidth={2} />
              <span className="text-[13px] pointer-events-none max-[500px]:hidden">{profile}</span>
              <select
                value={profile}
                onChange={(e) => onProfileChange(e.target.value as 'Code' | 'Ask' | 'Plan')}
                className="absolute inset-0 w-full h-full opacity-0 cursor-pointer appearance-none bg-transparent"
              >
                <option value="Code">Code</option>
                <option value="Ask">Ask</option>
                <option value="Plan">Plan</option>
              </select>
            </div>

            {/* Model selector hitbox — same pattern, no icon per the mockup. */}
            <div
              className="relative flex items-center gap-1.5 px-2 py-1.5 rounded-md cursor-pointer text-muted-foreground hover:bg-white/[0.04] hover:text-foreground transition-colors"
              title="Model"
            >
              <span className="text-[13px] pointer-events-none">
                {currentModel?.name ?? 'Model'}
              </span>
              <select
                value={effectiveModelId}
                onChange={(e) => onModelChange(e.target.value)}
                disabled={disabled || models.length === 0}
                className="absolute inset-0 w-full h-full opacity-0 cursor-pointer appearance-none bg-transparent disabled:cursor-not-allowed"
              >
                {models.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {/* Send / Stop — circular button in the actions row. Idle state is
              a transparent circle with a muted up-arrow; running state swaps
              to a destructive Stop button. */}
          {agentRunning ? (
            <button
              onClick={onStop}
              className="flex items-center justify-center w-7 h-7 rounded-full bg-destructive hover:bg-destructive/90 transition shrink-0"
              title="Stop"
              aria-label="Stop"
            >
              <Square className="w-3 h-3 text-destructive-foreground" />
            </button>
          ) : (
            <button
              onClick={onSend}
              disabled={!canSend}
              className="w-7 h-7 rounded-full flex items-center justify-center text-muted-foreground hover:bg-white/[0.06] hover:text-foreground transition shrink-0 disabled:opacity-50 disabled:cursor-not-allowed"
              title="Send message"
              aria-label="Send message"
            >
              <ArrowUp className="w-[15px] h-[15px]" strokeWidth={2} />
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
