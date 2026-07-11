import { useEffect, useRef, type WheelEvent } from 'react'
import { Plus, History, MoreHorizontal, Wifi, WifiOff, X, Loader2 } from 'lucide-react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/utils'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  DropdownMenuSub,
  DropdownMenuSubTrigger,
  DropdownMenuSubContent,
  DropdownMenuCheckboxItem,
} from '@/components/ui/dropdown-menu'
import {
  Tooltip,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import type { Session } from '@/types'

/**
 * Tab button variant — active vs inactive. Both keep a 3px top border so the
 * active indicator (blue top border) doesn't shift layout when it appears
 * (jitter prevention cushion, per the mockup). The right-edge separator
 * (`border-r border-border`) lives in the base classes so every tab gets one.
 */
const tabVariant = cva(
  'group relative flex items-center gap-1.5 h-full px-3 text-xs border-t-[3px] border-r border-border ' +
    'border-t-transparent transition-colors whitespace-nowrap shrink-0',
  {
    variants: {
      state: {
        inactive: 'text-muted-foreground hover:text-foreground',
        active: 'text-foreground border-t-primary bg-foreground/[0.01]',
      },
    },
    defaultVariants: { state: 'inactive' },
  },
)

type TabVariant = VariantProps<typeof tabVariant>

interface TabBarIconButtonProps {
  icon: React.ReactNode
  label: string
  onClick: () => void
  /** Extra classes (color/hover state) appended to the shared base classes. */
  className?: string
  ariaExpanded?: boolean
}

/**
 * TabBarIconButton — a Tooltip-wrapped icon button used in the right-side
 * controls strip. Extracts the repeated Tooltip + TooltipTrigger + button
 * pattern shared by the new-chat and history toggle buttons.
 */
function TabBarIconButton({ icon, label, onClick, className, ariaExpanded }: TabBarIconButtonProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          onClick={onClick}
          className={cn('p-1.5 rounded-md transition', className)}
          aria-label={label}
          aria-expanded={ariaExpanded}
        >
          {icon}
        </button>
      </TooltipTrigger>
    </Tooltip>
  )
}

interface ChatTabBarProps {
  /** Sessions currently shown as tabs (already filtered to openTabIds). */
  openTabs: Session[]
  activeSessionId: string | null
  /** Session ids that currently have a turn in flight — shown with a spinner. */
  runningSessionIds: Set<string>
  onSelectSession: (id: string) => void
  onNewChat: () => void
  onCloseTab: (id: string) => void
  onToggleHistory: () => void
  historyOpen: boolean
  connected: boolean
  /** Hides the connection indicator on mobile (per the design — mobile users
   *  see the reconnect banner already). */
  isDesktop: boolean
  /** When true, render a transient "New chat" placeholder tab at the end of
   *  the strip. Shown after the user clicks "+" but before a real session is
   *  created (lazy session creation — the session is spawned on first send). */
  showNewChatTab?: boolean
  /** Closes the transient "New chat" placeholder tab. */
  onCloseNewChatTab?: () => void
  /** MCP server configs for the overflow menu's MCP servers sub-menu.
   *  Each entry has a name and enabled flag. */
  mcpServers?: { name: string; enabled: boolean }[]
  /** Toggle a single MCP server's enabled flag via PATCH /api/mcp/servers/{name}. */
  onToggleMcpServer?: (name: string, enabled: boolean) => void
  /** Whether a server toggle is currently in flight (shows loading state). */
  mcpTogglingServer?: string | null
  /** Slot for the ChatHistory popout — rendered inside the relative container
   *  so it can absolute-position below the bar. */
  children?: React.ReactNode
}

/**
 * Chat tab bar — top of the chat panel. Renders one tab per open session plus
 * right-side controls (+ / history / overflow / connection).
 *
 * Not a Radix Tabs primitive: tabs here switch the *data* behind a single
 * shared message area, not separate content panels per tab (see design §2).
 * Active indicator is a 3px blue top border; inactive tabs keep a transparent
 * 3px top border so layout doesn't jitter when the indicator appears.
 */
export function ChatTabBar({
  openTabs,
  activeSessionId,
  runningSessionIds,
  onSelectSession,
  onNewChat,
  onCloseTab,
  onToggleHistory,
  historyOpen,
  connected,
  isDesktop,
  showNewChatTab = false,
  onCloseNewChatTab,
  mcpServers,
  onToggleMcpServer,
  mcpTogglingServer,
  children,
}: ChatTabBarProps) {
  const tabListRef = useRef<HTMLDivElement>(null)
  const activeTabRef = useRef<HTMLButtonElement>(null)

  // Scroll the active tab into view whenever it changes so the active
  // conversation is always visible without scrolling the whole strip.
  useEffect(() => {
    activeTabRef.current?.scrollIntoView({ inline: 'nearest', block: 'nearest' })
  }, [activeSessionId])

  // Wheel-to-scroll: vertical wheel scrolls the horizontal tab strip so a
  // trackpad/mouse user can reach off-screen tabs without a touch gesture.
  const handleWheel = (e: WheelEvent<HTMLDivElement>) => {
    if (e.deltaY === 0) return
    const el = tabListRef.current
    if (!el) return
    el.scrollLeft += e.deltaY
    e.preventDefault()
  }

  return (
    <div className="relative shrink-0 border-b border-border bg-panel">
      <div className="flex items-stretch h-10">
        {/* Tab strip — horizontally scrollable with hidden scrollbars. The
            right-side controls live in a sibling shrink-0 container so they
            never scroll away. */}
        <div
          ref={tabListRef}
          onWheel={handleWheel}
          className="flex items-stretch overflow-x-auto overflow-y-hidden hide-scrollbar min-w-0 flex-1"
        >
          {openTabs.length === 0 && !showNewChatTab && (
            <div className="flex items-center px-3 text-xs text-muted-foreground">
              No open chats
            </div>
          )}
          {openTabs.map((session) => {
            const isActive = session.id === activeSessionId
            const isRunning = runningSessionIds.has(session.id)
            const variant: TabVariant['state'] = isActive ? 'active' : 'inactive'
            return (
              <button
                key={session.id}
                ref={isActive ? activeTabRef : undefined}
                onClick={() => onSelectSession(session.id)}
                className={tabVariant({ state: variant })}
                title={session.name}
                aria-current={isActive ? 'page' : undefined}
              >
                {isRunning && (
                  <Loader2 className="w-3 h-3 animate-spin text-primary shrink-0" />
                )}
                <span className="max-w-[7rem] md:max-w-[10rem] truncate">
                  {session.name}
                </span>
                {/* Close-on-hover X — appears for every tab, including the
                    active one. Clicking hides the tab (does NOT delete the
                    session). stopPropagation so the tab click doesn't fire. */}
                <span
                  role="button"
                  tabIndex={-1}
                  onClick={(e) => {
                    e.stopPropagation()
                    onCloseTab(session.id)
                  }}
                  className="ml-1 flex items-center justify-center w-4 h-4 rounded text-muted-foreground hover:text-foreground hover:bg-accent opacity-0 group-hover:opacity-100 transition-opacity"
                  title="Close tab"
                  aria-label={`Close ${session.name}`}
                >
                  <X className="w-3 h-3" />
                </span>
              </button>
            )
          })}
          {/* Transient "New chat" placeholder tab — shown after the user clicks
              "+" but before a real session is created (lazy creation). Active
              whenever there is no real active session. Replaced by the real
              session's tab once the first message/attachment is sent. */}
          {showNewChatTab && (
            <button
              ref={!activeSessionId ? activeTabRef : undefined}
              onClick={onNewChat}
              className={tabVariant({ state: !activeSessionId ? 'active' : 'inactive' })}
              title="New chat"
              aria-current={!activeSessionId ? 'page' : undefined}
            >
              <span className="max-w-[7rem] md:max-w-[10rem] truncate italic text-muted-foreground">
                New chat
              </span>
              {onCloseNewChatTab && (
                <span
                  role="button"
                  tabIndex={-1}
                  onClick={(e) => {
                    e.stopPropagation()
                    onCloseNewChatTab()
                  }}
                  className="ml-1 flex items-center justify-center w-4 h-4 rounded text-muted-foreground hover:text-foreground hover:bg-accent opacity-0 group-hover:opacity-100 transition-opacity"
                  title="Close tab"
                  aria-label="Close New chat"
                >
                  <X className="w-3 h-3" />
                </span>
              )}
            </button>
          )}
        </div>

        {/* Right-side controls — never scroll away. */}
        <div className="flex items-center gap-1 px-2 shrink-0 border-l border-border">
          <TabBarIconButton
            icon={<Plus className="w-4 h-4" />}
            label="New chat"
            onClick={onNewChat}
            className="text-muted-foreground hover:text-foreground hover:bg-accent"
          />

          <TabBarIconButton
            icon={<History className="w-4 h-4" />}
            label="Chat history"
            onClick={onToggleHistory}
            className={
              historyOpen
                ? 'text-foreground bg-accent'
                : 'text-muted-foreground hover:text-foreground hover:bg-accent'
            }
            ariaExpanded={historyOpen}
          />

          {/* Overflow menu — MCP servers sub-menu (design §4) + a disabled
              "Export conversation" placeholder. */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                className="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition"
                aria-label="More options"
              >
                <MoreHorizontal className="w-4 h-4" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuLabel>Chat options</DropdownMenuLabel>
              <DropdownMenuSeparator />
              <DropdownMenuSub>
                <DropdownMenuSubTrigger>MCP servers</DropdownMenuSubTrigger>
                <DropdownMenuSubContent>
                  {(!mcpServers || mcpServers.length === 0) && (
                    <DropdownMenuItem disabled>No servers configured</DropdownMenuItem>
                  )}
                  {mcpServers?.map((server) => (
                    <DropdownMenuCheckboxItem
                      key={server.name}
                      checked={server.enabled}
                      onSelect={(e) => {
                        e.preventDefault()
                        onToggleMcpServer?.(server.name, !server.enabled)
                      }}
                    >
                      {mcpTogglingServer === server.name ? (
                        <Loader2 className="w-3.5 h-3.5 animate-spin" />
                      ) : null}
                      <span className="ml-2">{server.name}</span>
                    </DropdownMenuCheckboxItem>
                  ))}
                </DropdownMenuSubContent>
              </DropdownMenuSub>
              <DropdownMenuSeparator />
              <DropdownMenuItem disabled>Export conversation</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          {/* Connection indicator — hidden on mobile (mobile users see the
              reconnect banner already, per design §6). */}
          {isDesktop && (
            <span
              className="flex items-center gap-1 text-[11px] shrink-0 ml-1"
              title={connected ? 'Connected to daemon' : 'Disconnected — reconnecting'}
              aria-label={connected ? 'Connected' : 'Disconnected'}
            >
              {connected ? (
                <Wifi className="w-3.5 h-3.5 text-green-500" />
              ) : (
                <WifiOff className="w-3.5 h-3.5 text-red-500" />
              )}
            </span>
          )}
        </div>
      </div>

      {/* Slot for the ChatHistory popout — positioned absolutely below this
          bar via its own `absolute top-full` styling. */}
      {children}
    </div>
  )
}
