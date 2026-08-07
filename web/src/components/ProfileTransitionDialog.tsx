import { useState } from 'react'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import type { ProfileTransitionPreview } from '@/lib/api'
import { cn } from '@/lib/utils'

/**
 * What the user chose to do about a profile whose MCP server access differs.
 *
 * `history` and `fresh` map to the transition endpoint; `instructions` is the
 * ordinary in-place profile switch, which deliberately leaves server access
 * alone. `cancel` is not a value here — it closes the dialog and changes
 * nothing.
 */
export type ProfileTransitionChoice = 'history' | 'fresh' | 'instructions'

interface ProfileTransitionDialogProps {
  open: boolean
  /** Display label of the profile the user picked. */
  targetProfileLabel: string
  /** Effective server sets from the backend, or `null` while loading. */
  preview: ProfileTransitionPreview | null
  onConfirm: (choice: ProfileTransitionChoice) => void
  onCancel: () => void
  /** Disables the controls while a transition is in flight. */
  busy?: boolean
}

const CHOICES: {
  value: ProfileTransitionChoice
  title: string
  description: string
}[] = [
  {
    value: 'history',
    title: 'Start a new agent session with history',
    description:
      "Your conversation will be carried into the new session's first prompt. The new profile's instructions and MCP server access apply there.",
  },
  {
    value: 'fresh',
    title: 'Start a fresh conversation',
    description:
      'Open a blank conversation with this profile. This conversation stays unchanged.',
  },
  {
    value: 'instructions',
    title: 'Apply instructions only',
    description:
      "Use this profile's instructions in the current conversation. Its existing MCP server access stays the same.",
  },
]

/** Render a server set for display, naming the empty case explicitly. */
function serverSummary(servers: string[]): string {
  return servers.length > 0 ? servers.join(', ') : 'none'
}

/**
 * Profile-transition dialog — shown when the selected profile would change the
 * session's MCP server access.
 *
 * ACP fixes a session's MCP server list when the agent session starts, so that
 * access cannot change in place. Rather than switch silently and let the
 * selector imply access the session does not have, the user picks a lifecycle
 * operation. Shown only when the backend preview reports
 * `requiresNewSession` — an identical server set takes the ordinary in-place
 * path with no interruption.
 */
export function ProfileTransitionDialog({
  open,
  targetProfileLabel,
  preview,
  onConfirm,
  onCancel,
  busy,
}: ProfileTransitionDialogProps) {
  // Callers remount this component per transition (see the `key` in ChatPanel),
  // so the default choice resets naturally without a state-syncing effect.
  const [choice, setChoice] = useState<ProfileTransitionChoice>('history')

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onCancel()
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Tool access changes with a new session</DialogTitle>
          <DialogDescription>
            <span className="font-semibold text-foreground">
              {targetProfileLabel}
            </span>{' '}
            changes both instructions and MCP server access. ACP sets MCP servers
            when an agent session starts, so this conversation&rsquo;s server
            access cannot change in place.
          </DialogDescription>
        </DialogHeader>

        {preview && (
          <dl className="rounded-md border border-border bg-muted/40 p-3 text-xs">
            <div className="flex gap-2">
              <dt className="shrink-0 text-muted-foreground">Now:</dt>
              <dd className="text-foreground">
                {serverSummary(preview.currentServers)}
              </dd>
            </div>
            <div className="mt-1 flex gap-2">
              <dt className="shrink-0 text-muted-foreground">
                With {targetProfileLabel}:
              </dt>
              <dd className="text-foreground">
                {serverSummary(preview.targetServers)}
              </dd>
            </div>
          </dl>
        )}

        <fieldset disabled={busy} className="space-y-2">
          <legend className="sr-only">
            How to apply the {targetProfileLabel} profile
          </legend>
          {CHOICES.map((option) => (
            <label
              key={option.value}
              className={cn(
                'flex gap-3 rounded-md border p-3 text-sm',
                'cursor-pointer transition-colors',
                'has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring',
                choice === option.value
                  ? 'border-primary bg-accent/50'
                  : 'border-border hover:bg-accent/30',
                busy && 'cursor-not-allowed opacity-60',
              )}
            >
              <input
                type="radio"
                name="profile-transition-strategy"
                value={option.value}
                checked={choice === option.value}
                onChange={() => setChoice(option.value)}
                className="mt-0.5 size-4 shrink-0 accent-primary"
              />
              <span className="space-y-1">
                <span className="block font-medium text-foreground">
                  {option.title}
                </span>
                <span className="block text-xs text-muted-foreground">
                  {option.description}
                </span>
              </span>
            </label>
          ))}
        </fieldset>

        <DialogFooter>
          <Button variant="secondary" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={() => onConfirm(choice)} disabled={busy}>
            {busy ? 'Applying…' : 'Continue'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
