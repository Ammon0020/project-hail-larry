import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ChevronDown, ChevronRight, Copy, Crosshair, GitBranch, GitCommit, List, Loader2, RefreshCw, SquareArrowOutUpRight } from 'lucide-react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { api, type CommitDiffResult, type LogCommit } from '@/lib/api'
import { cn } from '@/lib/utils'
import { layoutGitGraph, type GitGraphLayout } from './gitGraphLayout'
import {
  buildContinuationVerticals,
  buildRowSegments,
  DOT_Y,
  DOT_RADIUS,
  HEAD_DOT_RADIUS,
  MERGE_DOT_RADIUS,
  graphWidth,
  laneX,
} from './gitGraphSvg'
import { CommitFileList } from './CommitFileList'
import { CommitContextMenu, type CommitContextMenuItem } from './CommitContextMenu'

const ROW_HEIGHT = 44
const DEFAULT_HEIGHT = 260
const MIN_HEIGHT = 160
const MAX_HEIGHT = 480

function relativeTime(iso: string): string {
  const timestamp = Date.parse(iso)
  if (!Number.isFinite(timestamp)) return iso
  const seconds = Math.max(0, Math.floor((Date.now() - timestamp) / 1000))
  if (seconds < 60) return 'just now'
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days}d ago`
  const months = Math.floor(days / 30)
  if (months < 12) return `${months}mo ago`
  return `${Math.floor(months / 12)}y ago`
}

/** Stable color class per branch lineage — cycled from a small semantic palette.
 *  Coloring by lineage (not lane index) keeps colors stable when lanes are
 *  reused and across pagination appends. */
const LANE_COLORS = [
  'text-primary',
  'text-sky-500',
  'text-emerald-500',
  'text-amber-500',
  'text-violet-500',
  'text-rose-500',
  'text-cyan-500',
  'text-orange-500',
]
function lineageColor(lineageId: number): string {
  return LANE_COLORS[lineageId % LANE_COLORS.length]
}

function GraphRow({
  commit,
  nodeIndex,
  layout,
  isExpanded,
  flatMode,
  onToggle,
  contextItems,
}: {
  commit: LogCommit
  nodeIndex: number
  layout: GitGraphLayout
  isExpanded: boolean
  flatMode: boolean
  onToggle: () => void
  contextItems: CommitContextMenuItem[]
}) {
  const graphNode = layout.nodes[nodeIndex]
  const segments = useMemo(
    () => (graphNode ? buildRowSegments(graphNode, ROW_HEIGHT) : null),
    [graphNode],
  )
  const gWidth = graphWidth(layout.laneCount)
  const dotX = graphNode ? laneX(graphNode.lane) : laneX(0)
  const dotR = commit.isHead
    ? HEAD_DOT_RADIUS
    : segments?.dot.isMerge
      ? MERGE_DOT_RADIUS
      : DOT_RADIUS

  return (
    <CommitContextMenu items={contextItems}>
      <div
        role="row"
        tabIndex={0}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            onToggle()
          }
        }}
        className="group flex w-full items-center gap-2 border-b border-border/40 px-2 text-left hover:bg-accent/40 focus-visible:bg-accent/40 focus-visible:outline-none"
        style={{ height: ROW_HEIGHT }}
        title={`${commit.message} (${commit.oid})`}
      >
        {flatMode && <div style={{ width: 12 }} className="shrink-0" />}
        {!flatMode && segments && (
          <svg
            width={gWidth}
            height={ROW_HEIGHT}
            className="shrink-0 overflow-visible"
            aria-hidden="true"
          >
            {/* Through-lane and commit-lane verticals */}
            {segments.verticals.map((v, idx) => (
              <line
                key={`v-${idx}-${v.lane}`}
                x1={laneX(v.lane)}
                y1={v.y0}
                x2={laneX(v.lane)}
                y2={v.y1}
                stroke="currentColor"
                strokeWidth="1.5"
                className={cn('opacity-70', lineageColor(v.lineageId))}
              />
            ))}
            {/* Parent-edge curves and truncated stubs — one per parent edge */}
            {segments.curves.map((c) => {
              const fromX = laneX(c.fromLane)
              const toX = laneX(c.toLane)
              if (c.dashed) {
                return (
                  <line
                    key={c.edgeId}
                    x1={fromX}
                    y1={c.y0}
                    x2={toX}
                    y2={c.y1}
                    stroke="currentColor"
                    strokeWidth="1.5"
                    strokeDasharray="3 3"
                    className="text-muted-foreground/50"
                  />
                )
              }
              // Bezier curve from dot to parent lane at row bottom
              const midY = (c.y0 + c.y1) / 2
              return (
                <path
                  key={c.edgeId}
                  d={`M ${fromX} ${c.y0} C ${fromX} ${midY}, ${toX} ${midY}, ${toX} ${c.y1}`}
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  className={cn('opacity-70', lineageColor(c.lineageId))}
                />
              )
            })}
            {/* Commit dot */}
            <circle
              cx={dotX}
              cy={DOT_Y}
              r={dotR}
              fill="currentColor"
              className={commit.isHead ? 'text-primary' : lineageColor(graphNode?.lineageId ?? 0)}
            />
            {commit.isHead && (
              <circle
                cx={dotX}
                cy={DOT_Y}
                r={HEAD_DOT_RADIUS + 2.5}
                fill="none"
                stroke="currentColor"
                strokeWidth="1"
                className="text-primary/50"
              />
            )}
          </svg>
        )}
        <div className="min-w-0 flex-1 py-1">
          <div className="flex min-w-0 items-center gap-1.5">
            <span className="truncate text-xs font-medium text-foreground">
              {commit.message || '(no subject)'}
            </span>
          </div>
          <div className="flex items-center gap-1.5 text-[10px] text-muted-foreground">
            <span className="font-mono">{commit.oid.slice(0, 7)}</span>
            <span>·</span>
            <span className="truncate">{relativeTime(commit.author.time)}</span>
            {commit.branchLabels.map((label) => (
              <span
                key={label}
                className="shrink-0 rounded-full bg-primary/15 px-1.5 py-0.5 text-[9px] font-medium text-primary"
              >
                {label}
              </span>
            ))}
          </div>
        </div>
        <ChevronRight
          className={cn(
            'h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform',
            isExpanded && 'rotate-90',
          )}
        />
      </div>
    </CommitContextMenu>
  )
}

/** Collapsible, resizable, virtualized history pane below current changes. */
export function GitHistorySection({
  workspaceId,
  onOpenCommitDiff,
  refreshKey = 0,
}: {
  workspaceId: string
  onOpenCommitDiff: (commitOid: string) => void
  refreshKey?: string | number
}) {
  const [expanded, setExpanded] = useState(true)
  const [flatMode, setFlatMode] = useState(false)
  const [height, setHeight] = useState(DEFAULT_HEIGHT)
  const [commits, setCommits] = useState<LogCommit[]>([])
  const [hasMore, setHasMore] = useState(false)
  const [loading, setLoading] = useState(true)
  const [loadingMore, setLoadingMore] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [expandedOid, setExpandedOid] = useState<string | null>(null)
  const [headFlash, setHeadFlash] = useState(false)
  const [headNotice, setHeadNotice] = useState<string | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const fetchToken = useRef(0)
  const pendingScrollHead = useRef<number | null>(null)
  const resizing = useRef(false)
  const diffCache = useRef<Map<string, CommitDiffResult>>(new Map())

  const refresh = useCallback(async () => {
    const token = ++fetchToken.current
    setLoading(true)
    setError(null)
    setExpandedOid(null)
    diffCache.current.clear()
    try {
      const result = await api.getGitLog(workspaceId)
      if (token !== fetchToken.current) return
      setCommits(result.commits)
      setHasMore(result.hasMore)
    } catch (err) {
      if (token === fetchToken.current) setError(err instanceof Error ? err.message : String(err))
    } finally {
      if (token === fetchToken.current) setLoading(false)
    }
  }, [workspaceId])

  useEffect(() => {
    void refresh()
    const tokenRef = fetchToken
    const token = tokenRef.current
    return () => {
      if (tokenRef.current === token) tokenRef.current++
    }
  }, [refresh, refreshKey])

  const loadMore = useCallback(async () => {
    if (loadingMore || !hasMore) return
    const token = fetchToken.current
    setLoadingMore(true)
    try {
      const result = await api.getGitLog(workspaceId, 100, commits.length)
      if (token !== fetchToken.current) return
      setCommits((current) => [
        ...current,
        ...result.commits.filter((commit) => !current.some((item) => item.oid === commit.oid)),
      ])
      setHasMore(result.hasMore)
    } catch (err) {
      if (token === fetchToken.current) setError(err instanceof Error ? err.message : String(err))
    } finally {
      if (token === fetchToken.current) setLoadingMore(false)
    }
  }, [commits.length, hasMore, loadingMore, workspaceId])

  // When a row is expanded, render the file list as a non-virtualized block
  // right after it. We adjust the virtualizer's count to account for the extra
  // "slot" so the total height stays correct. The expanded slot uses the same
  // absolute positioning as regular rows.
  const expandedIndex = expandedOid ? commits.findIndex((c) => c.oid === expandedOid) : -1
  const hasExpandedSlot = expandedIndex >= 0
  // Estimate the expanded file list height (capped at 160px).
  const expandedSlotHeight = 160

  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: commits.length + (hasMore ? 1 : 0) + (hasExpandedSlot ? 1 : 0),
    getScrollElement: () => scrollRef.current,
    estimateSize: (index: number) => {
      if (hasExpandedSlot && index === expandedIndex + 1) return expandedSlotHeight
      return ROW_HEIGHT
    },
    overscan: 8,
  })
  const layout = useMemo(() => layoutGitGraph(commits), [commits])
  const items = virtualizer.getVirtualItems()

  // Once commits update and a scroll is pending, jump to the index and flash.
  useEffect(() => {
    if (pendingScrollHead.current == null) return
    const index = pendingScrollHead.current
    pendingScrollHead.current = null
    const run = () => {
      virtualizer.scrollToIndex(index, { align: 'center' })
      setHeadFlash(true)
      window.setTimeout(() => setHeadFlash(false), 1500)
    }
    const raf = requestAnimationFrame(run)
    return () => cancelAnimationFrame(raf)
  }, [commits, virtualizer])

  // Scroll the virtualizer to the HEAD commit, fetching more pages if the
  // HEAD is outside the currently loaded window. Capped at 10 pages to avoid
  // unbounded fetching. Uses fetchToken so a concurrent `refresh` cancels us.
  const scrollToHead = useCallback(async () => {
    if (loading) return
    const token = fetchToken.current
    let current = commits
    let canLoad = hasMore
    for (let page = 0; page < 10; page++) {
      const headIndex = current.findIndex((c) => c.isHead)
      if (headIndex >= 0) {
        if (current === commits) {
          // HEAD already loaded — scroll directly (setCommits would be a no-op
          // since it's the same array reference, so the effect won't fire).
          virtualizer.scrollToIndex(headIndex, { align: 'center' })
          setHeadFlash(true)
          window.setTimeout(() => setHeadFlash(false), 1500)
        } else {
          // HEAD found after fetching more pages — setCommits triggers the
          // effect above which handles the scroll.
          pendingScrollHead.current = headIndex
          setCommits(current)
        }
        return
      }
      if (!canLoad) {
        setHeadNotice('HEAD not in recent history')
        window.setTimeout(() => setHeadNotice(null), 1800)
        return
      }
      const result = await api.getGitLog(workspaceId, 100, current.length)
      if (token !== fetchToken.current) return
      current = [
        ...current,
        ...result.commits.filter((c) => !current.some((x) => x.oid === c.oid)),
      ]
      canLoad = result.hasMore
      setCommits(current)
      setHasMore(canLoad)
    }
  }, [commits, hasMore, loading, virtualizer, workspaceId])

  useEffect(() => {
    const handlePointerMove = (event: PointerEvent) => {
      if (!resizing.current) return
      const next = Math.min(MAX_HEIGHT, Math.max(MIN_HEIGHT, window.innerHeight - event.clientY))
      setHeight(next)
    }
    const stopResize = () => {
      resizing.current = false
    }
    window.addEventListener('pointermove', handlePointerMove)
    window.addEventListener('pointerup', stopResize)
    return () => {
      window.removeEventListener('pointermove', handlePointerMove)
      window.removeEventListener('pointerup', stopResize)
    }
  }, [])

  const buildContextItems = useCallback(
    (commit: LogCommit): CommitContextMenuItem[] => [
      {
        label: 'Copy SHA',
        icon: <Copy className="h-3.5 w-3.5" />,
        onClick: () => void navigator.clipboard.writeText(commit.oid),
      },
      {
        label: 'Open diff in tab',
        icon: <SquareArrowOutUpRight className="h-3.5 w-3.5" />,
        onClick: () => onOpenCommitDiff(commit.oid),
      },
      ...(commit.branchLabels.length > 0
        ? [
            {
              label: `Checkout ${commit.branchLabels[0]}`,
              icon: <GitBranch className="h-3.5 w-3.5" />,
              onClick: () => {
                void api.gitCheckout(workspaceId, commit.branchLabels[0]).then(() => void refresh())
              },
            },
          ]
        : []),
      {
        label: 'Refresh',
        icon: <RefreshCw className="h-3.5 w-3.5" />,
        onClick: () => void refresh(),
      },
    ],
    [onOpenCommitDiff, refresh, workspaceId],
  )

  return (
    <section
      className="flex shrink-0 flex-col border-t border-border bg-background"
      style={{ height: expanded ? height : 34 }}
    >
      {expanded && (
        <div
          role="separator"
          aria-orientation="horizontal"
          aria-label="Resize history pane"
          tabIndex={0}
          className="h-1 shrink-0 cursor-row-resize border-b border-border/50 hover:bg-primary/30 focus-visible:bg-primary/30 focus-visible:outline-none"
          onPointerDown={() => {
            resizing.current = true
          }}
          onKeyDown={(event) => {
            if (event.key === 'ArrowUp') {
              event.preventDefault()
              setHeight((h) => Math.min(MAX_HEIGHT, h + 24))
            } else if (event.key === 'ArrowDown') {
              event.preventDefault()
              setHeight((h) => Math.max(MIN_HEIGHT, h - 24))
            }
          }}
        />
      )}
      <header className="flex h-8 shrink-0 items-center gap-1 border-b border-border px-2">
        <button
          type="button"
          onClick={() => setExpanded((value) => !value)}
          className="flex min-w-0 flex-1 items-center gap-1 text-left text-[10px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground"
          aria-expanded={expanded}
          aria-controls="git-history-content"
        >
          {expanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
          <GitBranch className="h-3 w-3" />
          <span>Graph</span>
          {!loading && <span className="font-normal normal-case tracking-normal">({commits.length})</span>}
          {headNotice && (
            <span className="ml-1 truncate text-[10px] font-normal normal-case tracking-normal text-muted-foreground">
              {headNotice}
            </span>
          )}
        </button>
        <button
          type="button"
          onClick={() => setFlatMode((value) => !value)}
          className="rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground"
          aria-label={flatMode ? 'Show graph view' : 'Show flat history'}
          title={flatMode ? 'Show graph view' : 'Show flat history'}
        >
          {flatMode ? <GitBranch className="h-3 w-3" /> : <List className="h-3 w-3" />}
        </button>
        <button
          type="button"
          onClick={() => void scrollToHead()}
          disabled={loading || !commits.some((c) => c.isHead) && !hasMore}
          className="rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground disabled:opacity-50"
          aria-label="Scroll to HEAD"
          title="Scroll to HEAD"
        >
          <Crosshair className="h-3 w-3" />
        </button>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={loading}
          className="rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground disabled:opacity-50"
          aria-label="Refresh history"
          title="Refresh history"
        >
          <RefreshCw className={cn('h-3 w-3', loading && 'animate-spin')} />
        </button>
      </header>
      {expanded && (
        <div ref={scrollRef} id="git-history-content" className="min-h-0 flex-1 overflow-y-auto">
          {loading ? (
            <div className="flex h-full items-center justify-center gap-1.5 text-xs text-muted-foreground">
              <Loader2 className="h-3.5 w-3.5 animate-spin" /> Loading history…
            </div>
          ) : error ? (
            <div className="flex h-full items-center justify-center px-3 text-center text-xs text-destructive">
              Failed to load history: {error}
            </div>
          ) : commits.length === 0 ? (
            <div className="flex h-full items-center justify-center gap-1.5 text-xs text-muted-foreground">
              <GitCommit className="h-3.5 w-3.5" /> No commits yet.
            </div>
          ) : (
            <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
              {items.map((item) => {
                // "Load more" sentinel row
                if (hasMore && item.index === commits.length + (hasExpandedSlot ? 1 : 0)) {
                  return (
                    <div
                      key="load-more"
                      className="absolute inset-x-0 flex h-11 items-center justify-center"
                      style={{ transform: `translateY(${item.start}px)` }}
                    >
                      <button
                        type="button"
                        onClick={() => void loadMore()}
                        disabled={loadingMore}
                        className="text-[10px] text-primary hover:underline disabled:opacity-50"
                      >
                        {loadingMore ? 'Loading…' : 'Load more history'}
                      </button>
                    </div>
                  )
                }
                // Expanded file-list slot (inserted after the expanded commit row)
                if (hasExpandedSlot && item.index === expandedIndex + 1) {
                  if (!expandedOid) return null
                  // The next node's incoming lanes are the active lines that
                  // must bridge the expanded file-list row.
                  const continuationLanes = layout.nodes[expandedIndex + 1]?.incomingLanes ?? []
                  const continuationVerticals = buildContinuationVerticals(continuationLanes, expandedSlotHeight)
                  return (
                    <div
                      key="expanded-files"
                      className={cn('absolute inset-x-0 top-0 flex gap-2', !flatMode && 'px-2')}
                      style={{ transform: `translateY(${item.start}px)` }}
                    >
                      {!flatMode && (
                        <svg
                          width={graphWidth(layout.laneCount)}
                          height={expandedSlotHeight}
                          className="shrink-0 overflow-visible"
                          aria-hidden="true"
                        >
                          {continuationVerticals.map((vertical) => (
                            <line
                              key={vertical.lane}
                              x1={laneX(vertical.lane)}
                              y1={vertical.y0}
                              x2={laneX(vertical.lane)}
                              y2={vertical.y1}
                              stroke="currentColor"
                              strokeWidth="1.5"
                              className={cn('opacity-70', lineageColor(vertical.lineageId))}
                            />
                          ))}
                        </svg>
                      )}
                      <div className="min-w-0 flex-1">
                        <CommitFileList
                          workspaceId={workspaceId}
                          commitOid={expandedOid}
                          cache={diffCache}
                          onOpenFile={onOpenCommitDiff}
                        />
                      </div>
                    </div>
                  )
                }
                // Regular commit row — adjust index if we're past the expanded slot
                const commitIndex = hasExpandedSlot && item.index > expandedIndex ? item.index - 1 : item.index
                const commit = commits[commitIndex]
                if (!commit) return null
                return (
                  <div
                    key={commit.oid}
                    className={cn(
                      'absolute inset-x-0 top-0',
                      commit.isHead && headFlash && 'bg-primary/20',
                    )}
                    style={{ transform: `translateY(${item.start}px)` }}
                  >
                    <GraphRow
                      commit={commit}
                      nodeIndex={commitIndex}
                      layout={layout}
                      isExpanded={expandedOid === commit.oid}
                      flatMode={flatMode}
                      onToggle={() => setExpandedOid((cur) => (cur === commit.oid ? null : commit.oid))}
                      contextItems={buildContextItems(commit)}
                    />
                  </div>
                )
              })}
            </div>
          )}
        </div>
      )}
    </section>
  )
}
