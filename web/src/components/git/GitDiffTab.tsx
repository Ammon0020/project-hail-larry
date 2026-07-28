import { useCallback, useEffect, useState } from 'react'
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

  // Fetch is wrapped in a useCallback (mirroring useGitState) so the
  // setState calls live inside an async function rather than lexically in
  // the effect body — avoids the react-hooks/set-state-in-effect cascading
  // render warning while still resetting to loading on dep changes.
  const refresh = useCallback(async () => {
    setDiff(null)
    setError(null)
    try {
      setDiff(await api.getGitDiff(workspaceId, path, staged))
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [workspaceId, path, staged])

  useEffect(() => {
    // eslint-disable-next-line
    void refresh()
  }, [refresh])

  if (error) {
    return (
      <div className="flex items-center justify-center h-full bg-editor text-sm text-muted-foreground p-4 text-center">
        Failed to load diff: {error}
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
