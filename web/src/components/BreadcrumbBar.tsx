import { ChevronRight, FolderCode, WrapText, Eye, Save } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * BreadcrumbBar — renders the active file's path relative to the workspace
 * root as a compact horizontal breadcrumb, sitting between the tab bar and
 * the editor content. The workspace display name is the first segment.
 *
 * Optional editor action buttons (Wrap / Preview / Save) can be rendered on
 * the right side when the relevant callbacks are supplied. They are only
 * passed in for editable text-file tabs, so the bar stays clean for other
 * uses (e.g. settings, browse previews).
 */
export function BreadcrumbBar({
  path,
  workspaceName,
  // Editor actions (optional — only shown for editable file tabs)
  wrap = false,
  onToggleWrap,
  showPreview = false,
  previewActive = false,
  onPreview,
  canSave = false,
  onSave,
}: {
  path: string
  workspaceName: string
  wrap?: boolean
  onToggleWrap?: () => void
  showPreview?: boolean
  previewActive?: boolean
  onPreview?: () => void
  canSave?: boolean
  onSave?: () => void
}) {
  if (!path || path === 'settings') return null

  const segments = path.split(/[\\/]/).filter(Boolean)
  if (segments.length === 0) return null

  const all = [workspaceName, ...segments]

  // Editor actions are only rendered when at least one callback is supplied,
  // so callers that just want the breadcrumb path get a clean bar.
  const hasActions = !!(onToggleWrap || onPreview || onSave)

  return (
    <div
      className={cn(
        'flex h-6 shrink-0 items-center gap-0.5 border-b border-background bg-panel px-3 text-[11px] select-none',
        'min-w-0',
      )}
    >
      {/* Breadcrumb path — takes the left side and truncates when space is tight. */}
      <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-hidden">
        {all.map((seg, i) => {
          const isLast = i === all.length - 1
          const isWorkspace = i === 0
          return (
            <span key={i} className="flex min-w-0 items-center gap-0.5">
              {i > 0 && <ChevronRight className="size-3 shrink-0 text-muted-foreground" />}
              {isWorkspace && <FolderCode className="size-3 shrink-0 text-primary" strokeWidth={1.5} />}
              <span
                className={cn(
                  'truncate',
                  isLast ? 'font-medium text-foreground' : 'text-muted-foreground',
                )}
              >
                {seg}
              </span>
            </span>
          )
        })}
      </div>

      {/* Editor Actions — Wrap, Preview (supported types), Save.
          Right-aligned; only rendered when callbacks are supplied. */}
      {hasActions && (
        <div className="ml-2 flex shrink-0 items-center gap-1.5">
          {onToggleWrap && (
            <button
              type="button"
              aria-label="Toggle line wrapping"
              aria-pressed={wrap}
              title="Toggle line wrapping"
              onClick={onToggleWrap}
              className={cn(
                'flex h-6 w-7 items-center justify-center rounded transition',
                wrap
                  ? 'bg-primary text-primary-foreground hover:bg-primary/90'
                  : 'bg-secondary text-secondary-foreground hover:bg-accent',
              )}
            >
              <WrapText className="size-3.5" />
            </button>
          )}
          {showPreview && onPreview && (
            <button
              type="button"
              aria-label={previewActive ? 'View raw source' : 'Preview'}
              aria-pressed={previewActive}
              title={previewActive ? 'View Raw' : 'Preview'}
              onClick={onPreview}
              className={cn(
                'flex h-6 items-center gap-1 rounded px-2 text-xs font-semibold transition',
                previewActive
                  ? 'bg-primary text-primary-foreground hover:bg-primary/90'
                  : 'bg-secondary text-secondary-foreground hover:bg-accent',
              )}
            >
              <Eye className="size-3.5" />
              <span className="hidden @md:inline">{previewActive ? 'Raw' : 'Preview'}</span>
            </button>
          )}
          {onSave && (
            <button
              type="button"
              onClick={canSave ? onSave : undefined}
              aria-disabled={!canSave}
              className={cn(
                'flex items-center gap-1.5 rounded px-2.5 py-1 text-xs font-semibold transition',
                canSave
                  ? 'bg-primary text-primary-foreground hover:bg-primary/90'
                  : 'cursor-default bg-secondary text-muted-foreground opacity-60',
              )}
            >
              <Save className="size-3" /> Save
            </button>
          )}
        </div>
      )}
    </div>
  )
}
