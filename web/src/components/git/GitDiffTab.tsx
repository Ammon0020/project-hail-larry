import { useCallback, useEffect, useRef, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { api, type GitDiffResult } from '@/lib/api'
import { GitDiffViewer } from './GitDiffViewer'

/**
 * Editor-tab wrapper that fetches a file's git diff and renders `GitDiffViewer`.
 *
 * Used by the editor tab dispatcher (EditorPane switches on `kind ===
 * 'git-diff'`). Future callers that already have base/head content — e.g. a
 * chat "edited files" popup showing an agent's proposed change — should render
 * `GitDiffViewer` directly with their own content rather than going through
 * this fetcher. That separation is the "single reusable diff component"
 * contract from the epic: the viewer is pure presentation, this wrapper is
 * the API binding.
 *
 * The diff is refetched whenever `path` or `staged` changes (reopening the
 * tab creates a fresh tab instance, so a mount-time fetch suffices).
 */
export function GitDiffTab({
  workspaceId,
  path,
  staged,
}: {
  workspaceId: string
  path: string
  staged: boolean
}) {
  const [diff, setDiff] = useState<GitDiffResult | null>(null)
  const [error, setError] = useState<string | null>(null)
  // Stale-fetch guard: bumped on every refresh so an in-flight promise can
  // tell whether it still owns the state slot. Prevents setState-on-unmounted
  // warnings and wrong-file diff flashes when the tab is switched mid-fetch.
  const fetchToken = useRef(0)

  // Fetch is wrapped in a useCallback (mirroring useGitState) so the
  // setState calls live inside an async function rather than lexically in
  // the effect body — avoids the react-hooks/set-state-in-effect cascading
  // render warning while still resetting to loading on dep changes.
  const refresh = useCallback(async () => {
    const token = ++fetchToken.current
    setDiff(null)
    setError(null)
    try {
      const result = await api.getGitDiff(workspaceId, path, staged)
      if (token === fetchToken.current) setDiff(result)
    } catch (err: unknown) {
      if (token === fetchToken.current) setError(err instanceof Error ? err.message : String(err))
    }
  }, [workspaceId, path, staged])

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh()
    // Invalidate any in-flight fetch from this run on unmount/dep change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    return () => { fetchToken.current++ }
  }, [refresh])

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center gap-3 h-full bg-editor text-sm text-muted-foreground p-4 text-center">
        <span>Failed to load diff: {error}</span>
        <button
          type="button"
          onClick={() => void refresh()}
          className="inline-flex items-center gap-1.5 rounded-md border border-border px-2 py-1 text-xs font-medium hover:bg-accent"
        >
          <RefreshCw className="h-3.5 w-3.5" /> Retry
        </button>
      </div>
    )
  }
  if (!diff) {
    return (
      <div className="flex items-center justify-center h-full bg-editor text-sm text-muted-foreground">
        Loading diff…
      </div>
    )
  }
  return (
    <GitDiffViewer
      path={path}
      base={diff.base}
      head={diff.head}
      truncated={diff.truncated}
    />
  )
}
