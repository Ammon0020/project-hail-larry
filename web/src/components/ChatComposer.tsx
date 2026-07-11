import { type KeyboardEvent } from 'react'
import { Paperclip, ArrowUp, Square, X, Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { Agent } from '@/types'

// Compact neutral pill styling shared by both selectors (design §3 — drop
// the old `border-primary/50 text-primary` accent on the model selector).
const selectorTriggerClass =
  'shrink-0 bg-background border border-input rounded-md text-xs px-2.5 py-1.5 ' +
  'text-muted-foreground hover:text-foreground hover:border-ring/60 h-auto font-medium'

interface CompactSelectProps {
  value: string
  onValueChange: (v: string) => void
  disabled?: boolean
  /** Used for both the trigger's title and aria-label. */
  label: string
  placeholder: string
  items: { id: string; name: string }[]
}

/**
 * CompactSelect — the neutral pill-styled Select used by the harness and model
 * pickers in the composer. Extracts the duplicated Select + SelectTrigger +
 * SelectContent boilerplate shared by the two selectors.
 */
function CompactSelect({
  value,
  onValueChange,
  disabled,
  label,
  placeholder,
  items,
}: CompactSelectProps) {
  return (
    <Select value={value} onValueChange={onValueChange} disabled={disabled}>
      <SelectTrigger className={selectorTriggerClass} title={label} aria-label={label}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {items.map((item) => (
          <SelectItem key={item.id} value={item.id}>{item.name}</SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

interface ChatComposerProps {
  agents: Agent[]
  effectiveAgentId: string
  effectiveModelId: string
  onAgentChange: (id: string) => void
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
}

/**
 * Chat input composer — textarea + attach + harness/model selectors + send/stop
 * grouped into one bordered card (design §3). The harness and model selectors
 * moved here from the old header so the "what you're about to send" controls
 * live next to the send button. Both selectors use neutral compact styling
 * (no `border-primary` accent — that cue is redundant inside the composer).
 */
export function ChatComposer({
  agents,
  effectiveAgentId,
  effectiveModelId,
  onAgentChange,
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
}: ChatComposerProps) {
  const currentAgent = agents.find((a) => a.id === effectiveAgentId)

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      onSend()
    }
  }

  return (
    <div className="p-2.5 lg:p-3 shrink-0 border-t border-border/50 pb-20 lg:pb-3">
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

      {/* Composer card — textarea on top, thin divider, actions row below. */}
      <div className="bg-input border border-border rounded-lg p-3 flex flex-col gap-3">
        <textarea
          value={input}
          onChange={(e) => onInputChange(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={agents.length === 0 ? 'Configure an agent first...' : 'Message agent...'}
          disabled={disabled}
          rows={2}
          className="w-full bg-transparent border-0 outline-none text-sm text-foreground placeholder:text-muted-foreground resize-none min-h-[2.5rem] disabled:opacity-60 disabled:cursor-not-allowed"
        />

        {/* Thin divider between textarea and actions. */}
        <div className="border-t border-white/5" />

        {/* Actions row: [attach] [harness] [model] ... [send/stop] */}
        <div className="flex items-center gap-2">
          <button
            onClick={onPickFiles}
            disabled={uploading || disabled}
            className="p-1.5 rounded-md bg-background border border-input text-muted-foreground hover:text-foreground hover:border-ring/60 transition shrink-0 disabled:opacity-60 disabled:cursor-not-allowed"
            title="Attach image"
            aria-label="Attach image"
          >
            {uploading ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Paperclip className="w-4 h-4" />
            )}
          </button>

          {/* Harness selector — pick agent (Blueprint Sec 5). */}
          <CompactSelect
            value={effectiveAgentId}
            onValueChange={onAgentChange}
            disabled={disabled}
            label="Agent harness"
            placeholder="Agent"
            items={agents}
          />

          {/* Model selector — neutral styling (no primary accent). */}
          <CompactSelect
            value={effectiveModelId}
            onValueChange={onModelChange}
            disabled={disabled || !currentAgent}
            label="Model"
            placeholder="Model"
            items={currentAgent?.models ?? []}
          />

          <div className="flex-1" />

          {/* Send / Stop — circular accent button in the actions row (mockup
              .send-btn). Stop takes the same slot with a destructive style. */}
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
              className={cn(
                'flex items-center justify-center w-7 h-7 rounded-full transition shrink-0',
                canSend
                  ? 'bg-primary hover:bg-primary/90'
                  : 'bg-muted cursor-not-allowed',
              )}
              title="Send message"
              aria-label="Send message"
            >
              <ArrowUp
                className={cn(
                  'w-3.5 h-3.5',
                  canSend ? 'text-primary-foreground' : 'text-muted-foreground',
                )}
              />
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
