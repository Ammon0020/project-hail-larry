import { useCallback, useEffect, useState } from 'react'
import { api, type GitRepoInfo } from '@/lib/api'

/**
 * Fetches the active workspace's git detection state (S-GIT-DETECT) and
 * refetches when the workspace id changes. Returns `null` while loading or
 * when no workspace is active. The caller gates git UI on `repoDetected`.
 */
export function useGitState(workspaceId: string | null | undefined) {
  const [gitState, setGitState] = useState<GitRepoInfo | null>(null)
  // Start in loading state when a workspace is active so the panel shows the
  // spinner instead of flashing the "No Git repository" screen before the
  // first detection effect runs.
  const [loading, setLoading] = useState(!!workspaceId)

  const refresh = useCallback(async () => {
    if (!workspaceId) {
      setGitState(null)
      return
    }
    setLoading(true)
    try {
      setGitState(await api.getGitState(workspaceId))
    } catch {
      // Read-only detection; a failure leaves the UI in the no-repo state
      // rather than throwing. The action bar item simply stays hidden.
      setGitState(null)
    } finally {
      setLoading(false)
    }
  }, [workspaceId])

  useEffect(() => {
    // eslint-disable-next-line
    void refresh()
  }, [refresh])

  return { gitState, loading, refresh }
}
