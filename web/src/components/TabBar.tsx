import { useEffect, useRef, useState } from 'react'
import { Circle, X, ChevronLeft, ChevronRight, RefreshCw, Settings as SettingsIcon, Eye } from 'lucide-react'
import { cn } from '@/lib/utils'
import { FileIcon } from '@/lib/fileIcon'
import type { Tab } from '@/types'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

/**
 * TabBar — open editor tabs and scroll controls. Reused in the desktop
 * header and the mobile EditorPane. Editor action buttons (Wrap / Preview /
 * Save) live in the BreadcrumbBar below the tabs.
 */
export function TabBar({
  tabs,
  activeTabId,
  onTabSelect,
  onTabClose,
  onCloseOthers,
  onCloseSaved,
  onCloseToRight,
  onCopyPath,
  onCopyRelativePath,
  onKeepOpen,
}: {
  tabs: Tab[]
  activeTabId: string | null
  onTabSelect: (id: string) => void
  onTabClose: (id: string) => void
  onCloseOthers?: (id: string) => void
  onCloseSaved?: (id: string) => void
  onCloseToRight?: (id: string) => void
  onCopyPath?: (path: string) => void
  onCopyRelativePath?: (path: string) => void
  onKeepOpen?: (id: string) => void
}) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const [canScrollLeft, setCanScrollLeft] = useState(false)
  const [canScrollRight, setCanScrollRight] = useState(false)

  // Context-menu state — `menuTabId` holds the id of the tab whose right-click
  // / long-press menu is currently open (null when closed). Controlled mode is
  // used so normal tab clicks (selection) keep working instead of being
  // captured by a Radix Trigger.
  const [menuTabId, setMenuTabId] = useState<string | null>(null)
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const clearLongPress = () => {
    if (longPressTimer.current) {
      clearTimeout(longPressTimer.current)
      longPressTimer.current = null
    }
  }

  useEffect(() => () => clearLongPress(), [])

  const measureScroll = () => {
    const el = scrollRef.current
    if (!el) return
    setCanScrollLeft(el.scrollLeft > 0)
    setCanScrollRight(el.scrollLeft < el.scrollWidth - el.clientWidth - 1)
  }

  useEffect(() => {
    const id = requestAnimationFrame(measureScroll)
    return () => cancelAnimationFrame(id)
  }, [tabs.length])

  // Automatically scroll the active tab into view when selected
  useEffect(() => {
    if (!activeTabId || !scrollRef.current) return
    
    // Defer to next tick to ensure newly added tabs are in the DOM and measured correctly
    const timeoutId = setTimeout(() => {
      const container = scrollRef.current
      if (!container) return
      
      const el = container.querySelector(`[data-tab-id="${CSS.escape(activeTabId)}"]`) as HTMLElement
      if (!el) return

      const containerRect = container.getBoundingClientRect()
      const elRect = el.getBoundingClientRect()

      // Calculate scroll offset if element is outside the visible container bounds
      if (elRect.left < containerRect.left) {
        container.scrollBy({ left: elRect.left - containerRect.left - 20, behavior: 'smooth' })
      } else if (elRect.right > containerRect.right) {
        container.scrollBy({ left: elRect.right - containerRect.right + 20, behavior: 'smooth' })
      }
    }, 0)
    
    return () => clearTimeout(timeoutId)
  }, [activeTabId, tabs.length])

  const scrollByTabs = (delta: number) => {
    const el = scrollRef.current
    if (!el) return
    el.scrollBy({ left: delta, behavior: 'smooth' })
  }

  return (
    <div className="flex h-auto w-full min-w-0 flex-col justify-center border-b border-background bg-panel @xl:h-9 @xl:flex-row @xl:items-center">
      {/* Top row (tabs) */}
      <div className="flex h-9 w-full min-w-0 flex-1 items-center @xl:w-auto">
        <button
          type="button"
          aria-label="Scroll tabs left"
          aria-hidden={!canScrollLeft}
          disabled={!canScrollLeft}
          onClick={() => scrollByTabs(-150)}
          className={cn(
            "flex h-9 w-5 shrink-0 items-center justify-center border-r border-background text-muted-foreground transition hover:bg-editor/50 hover:text-foreground",
            !canScrollLeft && "pointer-events-none opacity-0",
          )}
        >
          <ChevronLeft className="size-4" />
        </button>
        <div
          ref={scrollRef}
          onScroll={measureScroll}
          onWheel={(e) => {
            if (e.deltaY !== 0) {
              scrollRef.current?.scrollBy({ left: e.deltaY, behavior: 'smooth' })
            }
          }}
          className="tab-scrollbar flex h-full min-w-0 flex-1 overflow-x-auto"
          role="tablist"
        >
          {tabs.map((tab) => {
            const isActive = tab.id === activeTabId
            const isSettings = tab.kind === 'settings'
            const isBrowsePreview = tab.kind === 'preview'
            const canMenu = !isSettings
            const menuOpen = canMenu && menuTabId === tab.id
            const tabDiv = (
              <div
                key={tab.id}
                data-tab-id={tab.id}
                title={isSettings ? tab.name : tab.path}
                role="tab"
                aria-selected={isActive}
                tabIndex={0}
                className={cn(
                  'flex h-9 shrink-0 cursor-pointer items-center gap-2 border-r border-background px-3 text-sm select-none',
                  isActive
                    ? 'border-t-2 border-primary bg-editor text-foreground'
                    : 'bg-panel text-muted-foreground transition hover:bg-editor/50',
                )}
                onClick={() => onTabSelect(tab.id)}
                // Enter/Space activates the tab; arrow keys move focus between
                // sibling tabs (roving-tab style within the role="tablist").
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault()
                    onTabSelect(tab.id)
                  } else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
                    e.preventDefault()
                    const container = e.currentTarget.parentElement
                    if (!container) return
                    const els = Array.from(
                      container.querySelectorAll<HTMLElement>('[role="tab"]'),
                    )
                    const i = els.indexOf(e.currentTarget)
                    if (i < 0) return
                    const next =
                      e.key === 'ArrowRight'
                        ? (i + 1) % els.length
                        : (i - 1 + els.length) % els.length
                    els[next]?.focus()
                  }
                }}
                onContextMenu={
                  canMenu
                    ? (e) => {
                        e.preventDefault()
                        setMenuTabId(tab.id)
                      }
                    : undefined
                }
                onTouchStart={
                  canMenu
                    ? () => {
                        clearLongPress()
                        longPressTimer.current = setTimeout(() => {
                          setMenuTabId(tab.id)
                        }, 500)
                      }
                    : undefined
                }
                onTouchEnd={canMenu ? clearLongPress : undefined}
                onTouchMove={canMenu ? clearLongPress : undefined}
                onTouchCancel={canMenu ? clearLongPress : undefined}
              >
                {isSettings ? (
                  <SettingsIcon className="size-3.5 text-muted-foreground" />
                ) : isBrowsePreview ? (
                  <Eye className="size-3.5 text-muted-foreground" />
                ) : (
                  <FileIcon name={tab.name} className="size-3.5" />
                )}
                <span className={cn('max-w-30 truncate', tab.isPreview && 'font-normal italic')}>{tab.name}</span>
                {tab.unsaved && !isSettings && !isBrowsePreview && (
                  <Circle className="size-2 shrink-0 fill-primary text-primary" />
                )}
                {tab.changedOnDisk && !isSettings && !isBrowsePreview && (
                  <>
                    <RefreshCw className="size-3 shrink-0 text-warning" aria-hidden="true" />
                    <span className="sr-only">Changed on disk</span>
                  </>
                )}
                <button
                  type="button"
                  className="-mr-0.5 ml-1 flex size-4 shrink-0 items-center justify-center rounded-sm transition-all hover:size-5 hover:bg-muted"
                  onClick={(e) => {
                    e.stopPropagation()
                    onTabClose(tab.id)
                  }}
                  aria-label={`Close ${tab.name}`}
                >
                  <X className="size-3.5 text-muted-foreground hover:text-foreground" />
                </button>
              </div>
            )
            if (!canMenu) return tabDiv
            return (
              <DropdownMenu
                key={tab.id}
                open={menuOpen}
                onOpenChange={(o) => {
                  if (!o) setMenuTabId(null)
                }}
              >
                <DropdownMenuTrigger asChild>
                  {tabDiv}
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start">
                  <DropdownMenuItem onSelect={() => onTabClose(tab.id)}>
                    Close
                  </DropdownMenuItem>
                  {onCloseOthers && (
                    <DropdownMenuItem onSelect={() => onCloseOthers(tab.id)}>
                      Close Others
                    </DropdownMenuItem>
                  )}
                  {onCloseSaved && (
                    <DropdownMenuItem onSelect={() => onCloseSaved(tab.id)}>
                      Close Saved
                    </DropdownMenuItem>
                  )}
                  {onCloseToRight && (
                    <DropdownMenuItem onSelect={() => onCloseToRight(tab.id)}>
                      Close to the Right
                    </DropdownMenuItem>
                  )}
                  {onCopyPath && (
                    <DropdownMenuItem onSelect={() => onCopyPath(tab.path)}>
                      Copy Path
                    </DropdownMenuItem>
                  )}
                  {onCopyRelativePath && (
                    <DropdownMenuItem onSelect={() => onCopyRelativePath(tab.path)}>
                      Copy Relative Path
                    </DropdownMenuItem>
                  )}
                  {onKeepOpen && tab.isPreview && (
                    <DropdownMenuItem onSelect={() => onKeepOpen(tab.id)}>
                      Keep Open
                    </DropdownMenuItem>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            )
          })}
        </div>
        <button
          type="button"
          aria-label="Scroll tabs right"
          aria-hidden={!canScrollRight}
          disabled={!canScrollRight}
          onClick={() => scrollByTabs(150)}
          className={cn(
            "flex h-9 w-5 shrink-0 items-center justify-center text-muted-foreground transition hover:bg-editor/50 hover:text-foreground",
            !canScrollRight && "pointer-events-none opacity-0",
          )}
        >
          <ChevronRight className="size-4" />
        </button>
      </div>
    </div>
  )
}
