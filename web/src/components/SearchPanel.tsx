import { useState, useEffect, useRef, useMemo } from 'react'
import { Search, Loader2, CaseSensitive } from 'lucide-react'
import { cn } from '@/lib/utils'
import { api, type SearchResult } from '@/lib/api'

/**
 * Search panel — workspace-wide content search (Blueprint Sec 17 — left sidebar).
 *
 * Calls the backend GET /api/workspaces/{id}/search endpoint with a 300ms
 * debounce so rapid typing does not flood the server. Stale in-flight requests
 * are cancelled via an incrementing token so only the latest query's results
 * are rendered. Results are grouped by file path and the matched substring is
 * highlighted with a <mark>.
 */
export function SearchPanel({
  workspaceId,
  onSelectResult,
}: {
  /** Active workspace id, or null when no workspace is selected. */
  workspaceId: string | null
  /** Callback invoked when a result is clicked — opens the file in the editor. */
  onSelectResult?: (path: string, lineNumber: number) => void
}) {
  const [query, setQuery] = useState('')
  const [ignoreCase, setIgnoreCase] = useState(true)
  const [results, setResults] = useState<SearchResult[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Incrementing token used to ignore stale fetch responses. Each effect run
  // increments the ref; when a fetch resolves it checks its captured token
  // against the current value and discards the result if a newer query has
  // started.
  const reqTokenRef = useRef(0)

  // Focus the search input on mount so Ctrl+Shift+F lands the cursor in the
  // field. SearchPanel is conditionally rendered (mounted/unmounted when the
  // left panel switches), so this effect runs each time the panel opens.
  const inputRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  const trimmed = query.trim()

  // Clear stale results immediately when the input is emptied or the workspace
  // changes, so the user never sees results from a previous query. This is done
  // in the change handler (not in an effect) to avoid synchronous setState
  // inside an effect body (react-hooks/set-state-in-effect).
  const handleQueryChange = (value: string) => {
    setQuery(value)
    if (!value.trim()) {
      setResults([])
      setError(null)
      setLoading(false)
    }
  }

  // Clear results when switching workspaces (the old results belong to a
  // different root). Runs in the cleanup of the fetch effect below so it is not
  // a synchronous setState in the effect body.

  useEffect(() => {
    // No workspace or empty query — do not fetch. The render layer shows the
    // appropriate empty state; results from a prior query are cleared by the
    // change handler / cleanup below.
    if (!workspaceId || !trimmed) {
      return
    }

    // Debounce 300ms so typing a multi-character query does not fire one
    // request per keystroke.
    const timer = setTimeout(() => {
      const token = ++reqTokenRef.current
      setLoading(true)
      setError(null)
      api
        .searchWorkspace(workspaceId, {
          pattern: trimmed,
          ignoreCase,
          maxResults: 200,
        })
        .then((res) => {
          // Ignore the response if a newer query has started since this fetch.
          if (token !== reqTokenRef.current) return
          setResults(res)
          setLoading(false)
        })
        .catch((err: unknown) => {
          if (token !== reqTokenRef.current) return
          setError(err instanceof Error ? err.message : String(err))
          setResults([])
          setLoading(false)
        })
    }, 300)

    return () => {
      clearTimeout(timer)
      // When the workspace changes, drop results from the previous root so they
      // do not flash before the new search completes.
      setResults([])
      setError(null)
      setLoading(false)
    }
  }, [trimmed, workspaceId, ignoreCase])

  // Group results by file path for display. A Map preserves insertion order
  // (which is the backend's traversal order) and dedupes paths.
  const grouped = useMemo(() => {
    const map = new Map<string, SearchResult[]>()
    for (const r of results) {
      const list = map.get(r.path)
      if (list) list.push(r)
      else map.set(r.path, [r])
    }
    return Array.from(map.entries())
  }, [results])

  const showEmpty = !loading && !error && trimmed && results.length === 0

  return (
    <div className="flex flex-col h-full">
      <div className="px-3 py-2 text-[10px] font-semibold text-muted-foreground uppercase tracking-wider shrink-0">
        Search
      </div>
      <div className="px-3 pb-2 shrink-0">
        <label htmlFor="search-panel-input" className="sr-only">Search files</label>
        <div className="relative">
          <Search className="w-3.5 h-3.5 text-muted-foreground absolute left-2.5 top-1/2 -translate-y-1/2" />
          <input
            ref={inputRef}
            id="search-panel-input"
            type="text"
            value={query}
            onChange={(e) => handleQueryChange(e.target.value)}
            placeholder="Search files..."
            className="w-full bg-background border border-input rounded-md pl-8 pr-8 py-1.5 text-xs focus:outline-none focus:border-ring transition"
            aria-label="Search files"
            disabled={!workspaceId}
          />
          <button
            type="button"
            onClick={() => setIgnoreCase((v) => !v)}
            className={cn(
              'absolute right-2 top-1/2 -translate-y-1/2 p-0.5 rounded transition',
              ignoreCase
                ? 'text-muted-foreground hover:text-foreground'
                : 'text-primary bg-primary/10',
            )}
            aria-label="Toggle case sensitivity"
            aria-pressed={!ignoreCase}
            title={ignoreCase ? 'Case insensitive (click for exact case)' : 'Case sensitive (click for insensitive)'}
          >
            <CaseSensitive className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto px-3 pb-2 text-xs">
        {!workspaceId ? (
          <div className="text-muted-foreground text-center py-6">
            Select a workspace to search.
          </div>
        ) : loading ? (
          <div className="flex items-center justify-center gap-2 text-muted-foreground py-6">
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
            Searching...
          </div>
        ) : error ? (
          <div className="text-destructive text-center py-6 px-2 break-words">
            {error}
          </div>
        ) : showEmpty ? (
          <div className="text-muted-foreground text-center py-6">
            No results found.
          </div>
        ) : !trimmed ? (
          <div className="text-muted-foreground text-center py-6">
            Type to search across the workspace.
          </div>
        ) : (
          <div className="space-y-3">
            {grouped.map(([path, matches]) => (
              <div key={path}>
                <div className="text-muted-foreground font-medium truncate mb-1" title={path}>
                  {path}
                </div>
                <ul className="space-y-0.5">
                  {matches.map((r, i) => (
                    <li key={`${r.path}:${r.lineNumber}:${i}`}>
                      <button
                        type="button"
                        onClick={() => onSelectResult?.(r.path, r.lineNumber)}
                        className="w-full text-left flex gap-2 px-1.5 py-1 rounded hover:bg-primary/10 transition group"
                      >
                        <span className="text-muted-foreground tabular-nums shrink-0 group-hover:text-primary">
                          {r.lineNumber}
                        </span>
                        <span className="text-foreground truncate font-mono text-[11px]">
                          {renderLine(r)}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

/**
 * Renders a result line with the matched substring wrapped in a <mark>.
 * Uses byte offsets from the backend (matchStart/matchEnd) which correspond to
 * rune positions in the line content; since JS strings are UTF-16, we slice on
 * the string directly which is correct for BMP text. For astral characters the
 * offsets may be off, but this matches the common case and avoids expensive
 * rune conversion per line.
 */
function renderLine(r: SearchResult): React.ReactNode {
  const { lineContent, matchStart, matchEnd } = r
  if (matchStart < 0 || matchEnd > lineContent.length || matchStart >= matchEnd) {
    return lineContent
  }
  return (
    <>
      {lineContent.slice(0, matchStart)}
      <mark className="bg-yellow-500/30 text-foreground rounded px-0.5">
        {lineContent.slice(matchStart, matchEnd)}
      </mark>
      {lineContent.slice(matchEnd)}
    </>
  )
}
