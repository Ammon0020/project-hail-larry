import { useEffect, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { cn } from '@/lib/utils'

export interface CommitContextMenuItem {
  label: string
  icon?: ReactNode
  onClick: () => void
  disabled?: boolean
}

/** Lightweight right-click context menu for a commit row.
 *
 * Uses a portal positioned at the mouse coordinates — simpler than wiring
 * Radix DropdownMenu's trigger model (which opens on left-click). If this spike
 * graduates, swap for `@radix-ui/react-context-menu` for built-in focus
 * management and keyboard navigation. */
export function CommitContextMenu({
  items,
  children,
}: {
  items: CommitContextMenuItem[]
  children: ReactNode
}) {
  const [open, setOpen] = useState(false)
  const [pos, setPos] = useState({ x: 0, y: 0 })
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const handlePointerDown = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', handlePointerDown)
    document.addEventListener('keydown', handleEsc)
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown)
      document.removeEventListener('keydown', handleEsc)
    }
  }, [open])

  return (
    <>
      <div
        onContextMenu={(e) => {
          e.preventDefault()
          setPos({ x: e.clientX, y: e.clientY })
          setOpen(true)
        }}
      >
        {children}
      </div>
      {open &&
        createPortal(
          <div
            ref={ref}
            className="fixed z-50 min-w-[12rem] rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md animate-in fade-in-0 zoom-in-95"
            style={{ left: pos.x, top: pos.y }}
            role="menu"
          >
            {items.map((item, i) => (
              <button
                key={i}
                type="button"
                role="menuitem"
                disabled={item.disabled}
                onClick={() => {
                  item.onClick()
                  setOpen(false)
                }}
                className={cn(
                  'flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs outline-none select-none',
                  'hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground',
                  'disabled:pointer-events-none disabled:opacity-50',
                )}
              >
                {item.icon}
                {item.label}
              </button>
            ))}
          </div>,
          document.body,
        )}
    </>
  )
}


