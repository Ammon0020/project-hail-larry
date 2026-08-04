import { useEffect, useMemo, useRef, useState } from 'react'
import { Check, GitBranch, Search } from 'lucide-react'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { cn } from '@/lib/utils'

/** Normalized branch option: local branches win over remote-only names. */
export interface BranchOption {
  /** Value passed to `gitCheckout` (short name for remote-only). */
  name: string
  /** Display string, e.g. `origin/feature` for remote-only. */
  display: string
  /** True when this entry only exists on a remote. */
  isRemote: boolean
}

interface BranchPickerProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  branches: BranchOption[]
  currentBranch: string | null | undefined
  busy: boolean
  onCheckout: (branch: string) => void
}

/**
 * Searchable branch/checkout modal styled like a compact IDE picker.
 * Filters over both `display` and `name`; keyboard navigable; current branch
 * is shown with a check and is non-selectable. Escape closes via Radix Dialog.
 */
export function BranchPicker({
  open,
  onOpenChange,
  branches,
  currentBranch,
  busy,
  onCheckout,
}: BranchPickerProps) {
  const [query, setQuery] = useState('')
  const [activeIndex, setActiveIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)

  // Reset query + selection each time the dialog opens. State reset on a prop
  // transition is a legitimate effect use; the compiler rule is relaxed here
  // to match GitPanel's refresh effect.
  useEffect(() => {
    if (!open) return
    // Resetting picker state on open is a prop-transition reset, not a derived
    // value; the compiler rule is relaxed here as in GitPanel's refresh effect.
    /* eslint-disable react-hooks/set-state-in-effect */
    setQuery('')
    setActiveIndex(0)
    /* eslint-enable react-hooks/set-state-in-effect */
    // Focus after Radix mounts the portal content.
    const t = setTimeout(() => inputRef.current?.focus(), 0)
    return () => clearTimeout(t)
  }, [open])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return branches
    return branches.filter(
      (b) => b.display.toLowerCase().includes(q) || b.name.toLowerCase().includes(q),
    )
  }, [branches, query])

  // Derived active row: clamps to the filtered list without setState-in-render.
  const activeRow = filtered.length === 0 ? 0 : Math.min(activeIndex, filtered.length - 1)

  // Scroll the active row into view on keyboard navigation.
  useEffect(() => {
    if (!listRef.current) return
    const el = listRef.current.querySelector<HTMLElement>(`[data-idx="${activeRow}"]`)
    el?.scrollIntoView({ block: 'nearest' })
  }, [activeRow])

  const activate = (index: number) => {
    const branch = filtered[index]
    if (!branch || busy) return
    if (branch.name === currentBranch) return
    onCheckout(branch.name)
    onOpenChange(false)
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (filtered.length === 0) return
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setActiveIndex((activeRow + 1) % filtered.length)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActiveIndex((activeRow - 1 + filtered.length) % filtered.length)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      activate(activeRow)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={false}
        className="max-w-xl top-[10vh] translate-y-0 left-1/2 -translate-x-1/2 p-0 gap-0"
        onKeyDown={handleKeyDown}
      >
        <DialogTitle className="sr-only">Switch branch</DialogTitle>
        <div className="flex items-center gap-2 px-3 py-2 border-b border-border">
          <Search className="w-4 h-4 shrink-0 text-muted-foreground" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value)
              setActiveIndex(0)
            }}
            placeholder="Select a branch or tag to checkout"
            aria-controls="branch-picker-list"
            aria-activedescendant={filtered.length > 0 ? `branch-picker-item-${activeRow}` : undefined}
            className="flex-1 bg-transparent outline-none text-sm text-foreground placeholder:text-muted-foreground"
          />
        </div>
        <div
          ref={listRef}
          id="branch-picker-list"
          role="listbox"
          aria-label="Branches"
          className="max-h-[50vh] overflow-y-auto p-1"
        >
          {filtered.length === 0 ? (
            <div className="px-3 py-6 text-center text-sm text-muted-foreground">
              No branches found
            </div>
          ) : (
            filtered.map((b, i) => {
              const isCurrent = b.name === currentBranch
              const slashIdx = b.display.indexOf('/')
              const remotePrefix = b.isRemote && slashIdx >= 0 ? b.display.slice(0, slashIdx + 1) : ''
              const branchName = b.isRemote && slashIdx >= 0 ? b.display.slice(slashIdx + 1) : b.display
              return (
                <div
                  key={`${b.isRemote ? 'r' : 'l'}:${b.display}`}
                  id={`branch-picker-item-${i}`}
                  data-idx={i}
                  role="option"
                  aria-selected={i === activeRow}
                  aria-current={isCurrent ? 'true' : undefined}
                  aria-disabled={isCurrent || busy}
                  onMouseEnter={() => setActiveIndex(i)}
                  onClick={() => activate(i)}
                  className={cn(
                    'flex items-center gap-2 px-3 h-8 text-sm rounded-sm select-none',
                    i === activeRow ? 'bg-accent' : 'hover:bg-accent/50',
                    (isCurrent || busy) ? 'cursor-default opacity-90' : 'cursor-pointer',
                  )}
                >
                  <span className="flex w-4 shrink-0 items-center justify-center text-muted-foreground">
                    {isCurrent ? (
                      <Check className="h-3.5 w-3.5" />
                    ) : (
                      <GitBranch className="h-3.5 w-3.5 opacity-60" />
                    )}
                  </span>
                  <span className="min-w-0 truncate">
                    {remotePrefix && <span className="text-muted-foreground/60">{remotePrefix}</span>}
                    <span>{branchName}</span>
                  </span>
                </div>
              )
            })
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
