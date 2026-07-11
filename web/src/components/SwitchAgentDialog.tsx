import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Button } from '@/components/ui/button'

interface SwitchAgentDialogProps {
  open: boolean
  pendingAgentId: string | null
  currentAgentName: string
  /** Display name for the pending agent (falls back to its id by the caller). */
  pendingAgentName: string
  truncateLength: number
  setTruncateLength: (v: number) => void
  onConfirm: () => void
  onCancel: () => void
}

/**
 * Switch-agent confirmation dialog — shown when the user changes the agent
 * mid-conversation. Lets them pick how much of the prior conversation history
 * to transfer as context for the new agent. Extracted from ChatPanel to keep
 * the panel a slim orchestrator.
 */
export function SwitchAgentDialog({
  open,
  pendingAgentId,
  currentAgentName,
  pendingAgentName,
  truncateLength,
  setTruncateLength,
  onConfirm,
  onCancel,
}: SwitchAgentDialogProps) {
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onCancel()
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Switch Agent</DialogTitle>
          <DialogDescription>
            Switching from{' '}
            <span className="font-semibold text-foreground">
              {currentAgentName}
            </span>{' '}
            to{' '}
            <span className="font-semibold text-foreground">
              {pendingAgentName || pendingAgentId}
            </span>{' '}
            will start a fresh conversation. The previous conversation history will be
            transferred as context (truncated to{' '}
            <span className="font-semibold text-foreground">
              {truncateLength > 0
                ? `${truncateLength.toLocaleString()} chars`
                : 'no limit'}
            </span>
            ).
          </DialogDescription>
        </DialogHeader>

        {/* Truncate length control */}
        <div className="space-y-2">
          <label
            htmlFor="truncate-length"
            className="block text-xs font-medium text-muted-foreground"
          >
            Transfer history length
          </label>
          <Select
            value={String(truncateLength)}
            onValueChange={(v) => setTruncateLength(Number(v))}
          >
            <SelectTrigger
              id="truncate-length"
              className="w-full"
              aria-label="Transfer history length"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="4000">4,000 chars</SelectItem>
              <SelectItem value="8000">8,000 chars</SelectItem>
              <SelectItem value="16000">16,000 chars</SelectItem>
              <SelectItem value="32000">32,000 chars</SelectItem>
              <SelectItem value="0">Full (no limit)</SelectItem>
            </SelectContent>
          </Select>
        </div>

        <DialogFooter>
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button onClick={onConfirm}>Switch Agent</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
