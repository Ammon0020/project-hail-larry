import { useEffect, useRef, useState } from 'react'
import { FileCode, Circle, X, GitCompare, Save, ChevronLeft, ChevronRight, WrapText, RefreshCw, Settings as SettingsIcon } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { Tab } from '@/types'

/**
 * TabBar component — renders open editor tabs, scroll controls, and editor actions (Wrap, Diff, Save).
 * Reusable across the desktop header and the mobile EditorPane.
 */
export function TabBar({
  tabs,
  activeTabId,
  onTabSelect,
  onTabClose,
  onSave,
  wrap = false,
  onToggleWrap,
  showEditorActions = true,
}: {
  tabs: Tab[]
  activeTabId: string | null
  onTabSelect: (id: string) => void
  onTabClose: (id: string) => void
  onSave?: () => void
  wrap?: boolean
  onToggleWrap?: () => void
  showEditorActions?: boolean
}) {
  const activeTab = tabs.find((t) => t.id === activeTabId) || null

  const scrollRef = useRef<HTMLDivElement>(null)
  const [canScrollLeft, setCanScrollLeft] = useState(false)
  const [canScrollRight, setCanScrollRight] = useState(false)

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
    <div className="flex flex-col @xl:flex-row w-full min-w-0 @xl:items-center h-auto @xl:h-9 justify-center bg-panel border-b border-background">
      {/* Top row (tabs) */}
      <div className="flex flex-1 min-w-0 h-9 items-center w-full @xl:w-auto">
        {canScrollLeft && (
          <button
            type="button"
            aria-label="Scroll tabs left"
            onClick={() => scrollByTabs(-150)}
            className="flex items-center justify-center w-5 h-9 shrink-0 text-muted-foreground hover:text-foreground hover:bg-editor/50 transition border-r border-background"
          >
            <ChevronLeft className="w-4 h-4" />
          </button>
        )}
        <div
          ref={scrollRef}
          onScroll={measureScroll}
          onWheel={(e) => {
            if (e.deltaY !== 0) {
              scrollRef.current?.scrollBy({ left: e.deltaY, behavior: 'smooth' })
            }
          }}
          className="flex overflow-x-auto tab-scrollbar min-w-0 flex-1 h-full"
        >
          {tabs.map((tab) => {
            const isActive = tab.id === activeTabId
            return (
              <div
                key={tab.id}
                data-tab-id={tab.id}
                className={cn(
                  'flex items-center gap-2 px-3 h-9 text-sm shrink-0 border-r border-background cursor-pointer select-none',
                  isActive
                    ? 'bg-editor text-foreground border-t-2 border-primary'
                    : 'bg-panel text-muted-foreground hover:bg-editor/50 transition',
                )}
                onClick={() => onTabSelect(tab.id)}
              >
                {tab.kind === 'settings' ? (
                  <SettingsIcon className="w-3.5 h-3.5 text-muted-foreground" />
                ) : (
                  <FileCode className="w-3.5 h-3.5 text-yellow-400" />
                )}
                <span className="max-w-[120px] truncate">{tab.name}</span>
                {tab.unsaved && tab.kind !== 'settings' && (
                  <Circle className="w-2 h-2 text-primary fill-primary shrink-0" />
                )}
                {tab.changedOnDisk && tab.kind !== 'settings' && (
                  <>
                    <RefreshCw className="w-3 h-3 text-warning shrink-0" aria-hidden="true" />
                    <span className="sr-only">Changed on disk</span>
                  </>
                )}
                <X
                  className="w-3.5 h-3.5 text-muted-foreground hover:text-foreground cursor-pointer ml-1 shrink-0"
                  onClick={(e) => {
                    e.stopPropagation()
                    onTabClose(tab.id)
                  }}
                  aria-label={`Close ${tab.name}`}
                  role="button"
                />
              </div>
            )
          })}
        </div>
        {canScrollRight && (
          <button
            type="button"
            aria-label="Scroll tabs right"
            onClick={() => scrollByTabs(150)}
            className="flex items-center justify-center w-5 h-9 shrink-0 text-muted-foreground hover:text-foreground hover:bg-editor/50 transition"
          >
            <ChevronRight className="w-4 h-4" />
          </button>
        )}
      </div>
      
      {/* Editor Actions */}
      {showEditorActions && activeTab && activeTab.kind !== 'settings' && (
        <div className="flex gap-1.5 px-3 py-1 @xl:py-0 @xl:pl-1.5 items-center shrink-0 border-t border-border @xl:border-t-0 bg-panel @xl:bg-transparent">
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
          <button className="text-xs font-semibold bg-secondary hover:bg-accent text-secondary-foreground px-2.5 py-1 rounded transition flex items-center gap-1.5">
            <GitCompare className="w-3 h-3" /> Diff
          </button>
          {onSave && (
            <button
              onClick={onSave}
              className="text-xs font-semibold bg-primary hover:bg-primary/90 text-primary-foreground px-2.5 py-1 rounded flex items-center gap-1.5 transition"
            >
              <Save className="w-3 h-3" /> Save
            </button>
          )}
        </div>
      )}
    </div>
  )
}
