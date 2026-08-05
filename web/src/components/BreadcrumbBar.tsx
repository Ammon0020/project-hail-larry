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
        'flex items-center gap-0.5 px-3 h-6 text-[11px] bg-panel border-b border-background shrink-0 select-none',
        'min-w-0',
      )}
    >
      {/* Breadcrumb path — takes the left side and truncates when space is tight. */}
      <div className="flex items-center gap-0.5 min-w-0 flex-1 overflow-hidden">
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

      {/* Editor Actions — Wrap, Preview (supported types), Save.
          Right-aligned; only rendered when callbacks are supplied. */}
      {hasActions && (
        <div className="flex gap-1.5 items-center shrink-0 ml-2">
          {onToggleWrap && (
            <button
              type="button"
              aria-label="Toggle line wrapping"
              aria-pressed={wrap}
              title="Toggle line wrapping"
              onClick={onToggleWrap}
              className={cn(
                'flex items-center justify-center w-7 h-6 rounded transition',
                wrap
                  ? 'bg-primary text-primary-foreground hover:bg-primary/90'
                  : 'bg-secondary text-secondary-foreground hover:bg-accent',
              )}
            >
              <WrapText className="w-3.5 h-3.5" />
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
                'flex items-center gap-1 h-6 px-2 rounded text-xs font-semibold transition',
                previewActive
                  ? 'bg-primary text-primary-foreground hover:bg-primary/90'
                  : 'bg-secondary text-secondary-foreground hover:bg-accent',
              )}
            >
              <Eye className="w-3.5 h-3.5" />
              <span className="hidden @md:inline">{previewActive ? 'Raw' : 'Preview'}</span>
            </button>
          )}
          {onSave && (
            <button
              type="button"
              onClick={canSave ? onSave : undefined}
              aria-disabled={!canSave}
              className={cn(
                'text-xs font-semibold px-2.5 py-1 rounded flex items-center gap-1.5 transition',
                canSave
                  ? 'bg-primary hover:bg-primary/90 text-primary-foreground'
                  : 'bg-secondary text-muted-foreground cursor-default opacity-60',
              )}
            >
              <Save className="w-3 h-3" /> Save
            </button>
          )}
        </div>
      )}
    </div>
  )
}
