import { ChevronRight, FolderCode } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * BreadcrumbBar — renders the active file's path relative to the workspace
 * root as a compact horizontal breadcrumb, sitting between the tab bar and
 * the editor content. The workspace display name is the first segment.
 */
export function BreadcrumbBar({
  path,
  workspaceName,
}: {
  path: string
  workspaceName: string
}) {
  if (!path || path === 'settings') return null

  const segments = path.split(/[\\/]/).filter(Boolean)
  if (segments.length === 0) return null

  const all = [workspaceName, ...segments]

  return (
    <div
      className={cn(
        'flex items-center gap-0.5 px-3 h-6 text-[11px] bg-panel border-b border-background shrink-0 select-none',
        'overflow-hidden text-ellipsis whitespace-nowrap min-w-0',
      )}
    >
      {all.map((seg, i) => {
        const isLast = i === all.length - 1
        const isWorkspace = i === 0
        return (
          <span key={i} className="flex items-center gap-0.5 min-w-0">
            {i > 0 && <ChevronRight className="w-3 h-3 text-muted-foreground shrink-0" />}
            {isWorkspace && <FolderCode className="w-3 h-3 text-primary shrink-0" strokeWidth={1.5} />}
            <span
              className={cn(
                'truncate',
                isLast ? 'text-foreground font-medium' : 'text-muted-foreground',
              )}
            >
              {seg}
            </span>
          </span>
        )
      })}
    </div>
  )
}
