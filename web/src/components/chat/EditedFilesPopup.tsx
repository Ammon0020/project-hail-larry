import { useEffect, useId, useRef, useState } from 'react'
import { Check, ChevronDown, ChevronUp, FileEdit, Loader2, Undo2 } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { EditedFile } from '@/types'

interface EditedFilesPopupProps {
  /** Edited files for the active session. Empty = popup hidden. */
  editedFiles: EditedFile[]
  /** Called when the user accepts edits for a file (dismiss from list). */
  onAccept: (path: string) => void
  /** Called when the user reverts a file. Should call the revert API. */
  onRevert: (path: string) => void
  /** Called when the user clicks a file to open the diff viewer. */
  onOpenDiff: (path: string) => void
  /** Whether a revert is in progress for a path (shows spinner/disables). */
  revertingPath?: string | null
  /** Whether an accept is in progress for a path (shows spinner/disables). */
  acceptingPath?: string | null
  /** Restores focus to the composer after activating a row action. */
  onCloseFocus?: () => void
}

/** Compact composer popover for reviewing files changed by the active agent. */
export function EditedFilesPopup({
  editedFiles,
  onAccept,
  onRevert,
  onOpenDiff,
  revertingPath = null,
  acceptingPath = null,
  onCloseFocus,
}: EditedFilesPopupProps) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const toggleRef = useRef<HTMLButtonElement>(null)
  const firstFileRef = useRef<HTMLButtonElement>(null)
  const popupId = useId()
  const closeAfterAction = () => {
    setOpen(false)
    onCloseFocus?.()
  }

  useEffect(() => {
    if (!open) return
    firstFileRef.current?.focus()

    function handleClickOutside(event: MouseEvent) {
      const target = event.target as Element | null
      if (!target || !ref.current || ref.current.contains(target)) return
      if (typeof target.closest === 'function' && target.closest('[data-edited-files-toggle]')) {
        return
      }
      setOpen(false)
    }
    function handleEscape(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setOpen(false)
        toggleRef.current?.focus()
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    document.addEventListener('keydown', handleEscape)
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
      document.removeEventListener('keydown', handleEscape)
    }
  }, [open])

  if (editedFiles.length === 0) return null

  return (
    <div ref={ref} className="relative">
      <button
        ref={toggleRef}
        type="button"
        data-edited-files-toggle
        onClick={() => setOpen((value) => !value)}
        className={cn(
          'relative flex h-8 min-w-8 items-center justify-center gap-1 rounded-md bg-white/[0.04] px-1.5 text-muted-foreground transition hover:bg-white/[0.12] hover:text-foreground',
          open && 'bg-white/[0.12] text-foreground',
        )}
        title="Agent-edited files"
        aria-label={`${editedFiles.length} agent-edited file${editedFiles.length === 1 ? '' : 's'}`}
        aria-expanded={open}
        aria-controls={popupId}
        aria-haspopup="dialog"
      >
        <FileEdit className="size-3.5" strokeWidth={2.5} />
        <span className="text-xs font-medium">{editedFiles.length}</span>
        <span
          aria-hidden="true"
          className="absolute -top-0.5 -right-0.5 size-2 rounded-full border border-input bg-blue-500"
        />
        {open ? <ChevronUp className="size-3" /> : <ChevronDown className="size-3" />}
      </button>

      {open && (
        <div
          id={popupId}
          role="dialog"
          aria-label="Agent-edited files"
          className="absolute bottom-full left-0 z-50 mb-2 w-70 rounded-[10px] border border-border bg-panel p-2 shadow-lg"
        >
          <div className="border-b border-border px-1 pb-2 text-xs font-medium text-muted-foreground">
            Agent edits ({editedFiles.length})
          </div>
          <div role="list" className="mt-1 flex max-h-60 flex-col overflow-y-auto">
            {editedFiles.map((file, index) => {
              const fileName = file.path.split(/[\\/]/).pop() || file.path
              const reverting = revertingPath === file.path
              const accepting = acceptingPath === file.path
              const processing = accepting || reverting
              return (
                <div
                  key={file.path}
                  role="listitem"
                  className="flex min-w-0 items-center gap-1 rounded-md p-1 text-xs hover:bg-accent"
                >
                  <button
                    ref={index === 0 ? firstFileRef : undefined}
                    type="button"
                    onClick={() => {
                      onOpenDiff(file.path)
                      closeAfterAction()
                    }}
                    disabled={processing}
                    className="min-w-0 flex-1 truncate p-1 text-left text-foreground disabled:cursor-not-allowed disabled:opacity-50"
                    title={file.path}
                  >
                    {fileName}
                  </button>
                  <span className="shrink-0 font-mono text-[11px] whitespace-nowrap">
                    <span className="text-green-500">+{file.addedLines}</span>
                    <span className="text-destructive">/-{file.removedLines}</span>
                  </span>
                  <button
                    type="button"
                    onClick={() => {
                      onAccept(file.path)
                      closeAfterAction()
                    }}
                    disabled={processing}
                    className="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground transition hover:bg-background hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
                    title={`Accept edits to ${file.path}`}
                    aria-label={`Accept edits to ${file.path}`}
                  >
                    {accepting ? <Loader2 className="size-3.5 animate-spin" /> : <Check className="size-3.5" />}
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      if (window.confirm(`Revert agent edits to "${file.path}"?`)) {
                        onRevert(file.path)
                        closeAfterAction()
                      }
                    }}
                    disabled={processing}
                    className="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground transition hover:bg-background hover:text-destructive disabled:cursor-not-allowed disabled:opacity-50"
                    title={`Revert edits to ${file.path}`}
                    aria-label={`Revert edits to ${file.path}`}
                  >
                    {reverting ? <Loader2 className="size-3.5 animate-spin" /> : <Undo2 className="size-3.5" />}
                  </button>
                </div>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}
