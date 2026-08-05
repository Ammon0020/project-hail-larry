import { type ReactNode } from 'react'
import * as ContextMenu from '@radix-ui/react-context-menu'

export interface CommitContextMenuItem {
  label: string
  icon?: ReactNode
  onClick: () => void
  disabled?: boolean
}

/** Right-click context menu for a commit row.
 *
 * Wraps `@radix-ui/react-context-menu` for built-in keyboard navigation, focus
 * management, and Escape-to-close. The component interface is stable so callers
 * don't need to change when the underlying primitives evolve. */
export function CommitContextMenu({
  items,
  children,
}: {
  items: CommitContextMenuItem[]
  children: ReactNode
}) {
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content
          className="z-50 min-w-[12rem] rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md animate-in fade-in-0 zoom-in-95"
        >
          {items.map((item, i) => (
            <ContextMenu.Item
              key={i}
              disabled={item.disabled}
              onSelect={() => item.onClick()}
              className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs outline-none select-none hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground disabled:pointer-events-none disabled:opacity-50"
            >
              {item.icon}
              {item.label}
            </ContextMenu.Item>
          ))}
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  )
}
