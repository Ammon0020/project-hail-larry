import { cva, type VariantProps } from 'class-variance-authority'
import type { CommitFileStatus } from '@/lib/api/git'
import { cn } from '@/lib/utils'

const statusBadge = cva(
  'inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-[3px] text-[9px] font-bold leading-none',
  {
    variants: {
      status: {
        added: 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400',
        modified: 'bg-amber-500/15 text-amber-600 dark:text-amber-400',
        deleted: 'bg-rose-500/15 text-rose-600 dark:text-rose-400',
        renamed: 'bg-sky-500/15 text-sky-600 dark:text-sky-400',
      },
    },
  },
)

const statusLetter: Record<CommitFileStatus, string> = {
  added: 'A',
  modified: 'M',
  deleted: 'D',
  renamed: 'R',
}

export function CommitStatusBadge({
  status,
  className,
}: VariantProps<typeof statusBadge> & { status: CommitFileStatus; className?: string }) {
  return (
    <span className={cn(statusBadge({ status }), className)} aria-label={status}>
      {statusLetter[status]}
    </span>
  )
}
