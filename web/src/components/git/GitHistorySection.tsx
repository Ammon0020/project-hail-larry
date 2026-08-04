import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ChevronDown, ChevronRight, GitBranch, GitCommit, List, Loader2, RefreshCw } from 'lucide-react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { api, type LogCommit } from '@/lib/api'
import { cn } from '@/lib/utils'
import { layoutGitGraph } from './gitGraphLayout'

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

function GraphRow({ commit, lane, laneCount, edges, onOpen, flatMode }: {
  commit: LogCommit
  lane: number
  laneCount: number
  edges: ReturnType<typeof layoutGitGraph>['nodes'][number]['edges']
  onOpen: (oid: string) => void
  flatMode: boolean
}) {
  const laneWidth = 16
  const graphWidth = Math.max(28, laneCount * laneWidth + 12)
  const x = lane * laneWidth + 8
  return (
    <button
      type="button"
      onClick={() => onOpen(commit.oid)}
      className="group flex w-full items-center gap-2 border-b border-border/40 px-2 text-left hover:bg-accent/60 focus-visible:bg-accent/60 focus-visible:outline-none"
      style={{ height: ROW_HEIGHT }}
      title={`${commit.message} (${commit.oid})`}
    >
      {!flatMode && <svg width={graphWidth} height={ROW_HEIGHT} className="shrink-0 overflow-visible" aria-hidden="true">
        {edges.map((edge, index) => {
          const parentX = edge.parentLane === null ? x : edge.parentLane * laneWidth + 8
          const key = `${commit.oid}:${index}:${edge.parentIndex}`
          return edge.parentLane === lane ? (
            <line key={key} x1={x} y1="16" x2={parentX} y2={ROW_HEIGHT} stroke="currentColor" strokeWidth="1.5" className="text-primary/70" />
          ) : (
            <path
              key={key}
              d={`M ${x} 16 C ${x} 25, ${parentX} 25, ${parentX} ${ROW_HEIGHT}`}
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeDasharray={edge.offscreen ? '3 3' : undefined}
              className={edge.offscreen ? 'text-muted-foreground/50' : 'text-primary/70'}
            />
          )
        })}
        <circle cx={x} cy="16" r={commit.isHead ? 4.5 : 3.5} fill="currentColor" className={commit.isHead ? 'text-primary' : 'text-sky-500'} />
        {commit.isHead && <circle cx={x} cy="16" r="7" fill="none" stroke="currentColor" strokeWidth="1" className="text-primary/50" />}
      </svg>}
      <div className="min-w-0 flex-1 py-1">
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="truncate text-xs font-medium text-foreground">{commit.message || '(no subject)'}</span>
          {commit.branchLabels.map((label) => (
            <span key={label} className="shrink-0 rounded-full bg-primary/15 px-1.5 py-0.5 text-[9px] font-medium text-primary">{label}</span>
          ))}
        </div>
        <div className="flex items-center gap-1.5 text-[10px] text-muted-foreground">
          <span className="font-mono">{commit.oid.slice(0, 7)}</span>
          <span>·</span>
          <span className="truncate">{relativeTime(commit.author.time)}</span>
        </div>
      </div>
      <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
    </button>
  )
}

/** Collapsible, resizable, virtualized history pane below current changes. */
export function GitHistorySection({ workspaceId, onOpenCommitDiff, refreshKey = 0 }: {
  workspaceId: string
  onOpenCommitDiff: (oid: string) => void
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
  const scrollRef = useRef<HTMLDivElement>(null)
  const fetchToken = useRef(0)
  const resizing = useRef(false)

  const refresh = useCallback(async () => {
    const token = ++fetchToken.current
    setLoading(true)
    setError(null)
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
    // Guard against stale appends if the workspace changes mid-fetch; the
    // refresh effect bumps fetchToken on workspace change and resets commits.
    const token = fetchToken.current
    setLoadingMore(true)
    try {
      const result = await api.getGitLog(workspaceId, 100, commits.length)
      if (token !== fetchToken.current) return
      setCommits((current) => [...current, ...result.commits.filter((commit) => !current.some((item) => item.oid === commit.oid))])
      setHasMore(result.hasMore)
    } catch (err) {
      if (token === fetchToken.current) setError(err instanceof Error ? err.message : String(err))
    } finally {
      if (token === fetchToken.current) setLoadingMore(false)
    }
  }, [commits.length, hasMore, loadingMore, workspaceId])

  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: commits.length + (hasMore ? 1 : 0),
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
  })
  const layout = useMemo(() => layoutGitGraph(commits), [commits])
  const items = virtualizer.getVirtualItems()

  useEffect(() => {
    const handlePointerMove = (event: PointerEvent) => {
      if (!resizing.current) return
      const next = Math.min(MAX_HEIGHT, Math.max(MIN_HEIGHT, window.innerHeight - event.clientY))
      setHeight(next)
    }
    const stopResize = () => { resizing.current = false }
    window.addEventListener('pointermove', handlePointerMove)
    window.addEventListener('pointerup', stopResize)
    return () => {
      window.removeEventListener('pointermove', handlePointerMove)
      window.removeEventListener('pointerup', stopResize)
    }
  }, [])

  return (
    <section className="flex shrink-0 flex-col border-t border-border bg-background" style={{ height: expanded ? height : 34 }}>
      {expanded && (
        <div
          role="separator"
          aria-orientation="horizontal"
          aria-label="Resize history pane"
          tabIndex={0}
          className="h-1 shrink-0 cursor-row-resize border-b border-border/50 hover:bg-primary/30 focus-visible:bg-primary/30 focus-visible:outline-none"
          onPointerDown={() => { resizing.current = true }}
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
        <button type="button" onClick={() => setExpanded((value) => !value)} className="flex min-w-0 flex-1 items-center gap-1 text-left text-[10px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground" aria-expanded={expanded} aria-controls="git-history-content">
          {expanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
          <GitBranch className="h-3 w-3" />
          <span>Graph</span>
          {!loading && <span className="font-normal normal-case tracking-normal">({commits.length})</span>}
        </button>
        <button type="button" onClick={() => setFlatMode((value) => !value)} className="rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground" aria-label={flatMode ? 'Show graph view' : 'Show flat history'} title={flatMode ? 'Show graph view' : 'Show flat history'}>
          {flatMode ? <GitBranch className="h-3 w-3" /> : <List className="h-3 w-3" />}
        </button>
        <button type="button" onClick={() => void refresh()} disabled={loading} className="rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground disabled:opacity-50" aria-label="Refresh history" title="Refresh history">
          <RefreshCw className={cn('h-3 w-3', loading && 'animate-spin')} />
        </button>
      </header>
      {expanded && (
        <div ref={scrollRef} id="git-history-content" className="min-h-0 flex-1 overflow-y-auto">
          {loading ? (
            <div className="flex h-full items-center justify-center gap-1.5 text-xs text-muted-foreground"><Loader2 className="h-3.5 w-3.5 animate-spin" /> Loading history…</div>
          ) : error ? (
            <div className="flex h-full items-center justify-center px-3 text-center text-xs text-destructive">Failed to load history: {error}</div>
          ) : commits.length === 0 ? (
            <div className="flex h-full items-center justify-center gap-1.5 text-xs text-muted-foreground"><GitCommit className="h-3.5 w-3.5" /> No commits yet.</div>
          ) : (
            <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
              {items.map((item) => {
                if (item.index >= commits.length) {
                  return <div key="load-more" className="absolute inset-x-0 flex h-11 items-center justify-center" style={{ transform: `translateY(${item.start}px)` }}><button type="button" onClick={() => void loadMore()} disabled={loadingMore} className="text-[10px] text-primary hover:underline disabled:opacity-50">{loadingMore ? 'Loading…' : 'Load more history'}</button></div>
                }
                const node = layout.nodes[item.index]
                return <div key={commits[item.index].oid} className="absolute inset-x-0 top-0" style={{ transform: `translateY(${item.start}px)` }}><GraphRow commit={commits[item.index]} lane={node.lane} laneCount={layout.laneCount} edges={node.edges} onOpen={onOpenCommitDiff} flatMode={flatMode} /></div>
              })}
            </div>
          )}
        </div>
      )}
    </section>
  )
}
