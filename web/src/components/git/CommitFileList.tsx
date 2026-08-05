import { useEffect, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { api, type CommitDiffResult } from '@/lib/api'
import { CommitStatusBadge } from './CommitStatusBadge'

/** Compact inline list of changed files for a selected commit.
 *
 * Fetches via `getGitCommitDiff` on mount and caches the result in the parent's
 * `cache` Map so re-expanding is instant. Shows status badges (A/M/D/R) and
 * truncated paths. Clicking a file opens the full diff in an editor tab. */
export function CommitFileList({
  workspaceId,
  commitOid,
  cache,
  onOpenFile,
}: {
  workspaceId: string
  commitOid: string
  cache: React.MutableRefObject<Map<string, CommitDiffResult>>
  onOpenFile: (oid: string) => void
}) {
  const cached = cache.current.get(commitOid)
  const [result, setResult] = useState<CommitDiffResult | null>(cached ?? null)
  const [loading, setLoading] = useState(!cached)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (cached) return
    let cancelled = false
    api
      .getGitCommitDiff(workspaceId, commitOid)
      .then((res) => {
        if (cancelled) return
        cache.current.set(commitOid, res)
        setResult(res)
        setLoading(false)
      })
      .catch((err) => {
        if (cancelled) return
        setError(err instanceof Error ? err.message : String(err))
        setLoading(false)
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [commitOid, workspaceId, cached])

  if (loading) {
    return (
      <div className="flex items-center gap-1.5 px-3 py-2 text-[10px] text-muted-foreground">
        <Loader2 className="h-3 w-3 animate-spin" /> Loading changed files…
      </div>
    )
  }

  if (error) {
    return <div className="px-3 py-2 text-[10px] text-destructive">Failed to load: {error}</div>
  }

  if (!result || result.files.length === 0) {
    return <div className="px-3 py-2 text-[10px] text-muted-foreground">No file changes.</div>
  }

  return (
    <div className="max-h-40 overflow-y-auto border-b border-border/40 bg-muted/20 py-1">
      {result.files.map((file) => (
        <button
          key={file.path}
          type="button"
          onClick={() => onOpenFile(commitOid)}
          className="flex w-full items-center gap-1.5 px-3 py-0.5 text-left text-[11px] hover:bg-accent/60"
          title={file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}
        >
          <CommitStatusBadge status={file.status} />
          <span className="truncate font-mono text-muted-foreground">
            {file.oldPath ? (
              <>
                <span className="line-through opacity-60">{file.oldPath}</span>
                <span className="mx-0.5">→</span>
                <span>{file.path}</span>
              </>
            ) : (
              file.path
            )}
          </span>
        </button>
      ))}
    </div>
  )
}
