import { useCallback, useEffect, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { api, type CommitDiffResult } from '@/lib/api'
import { GitDiffViewer } from './GitDiffViewer'

/** Multi-file read-only diff view for a selected history commit. */
export function GitCommitDiffTab({ workspaceId, commitOid }: { workspaceId: string; commitOid: string }) {
  const [result, setResult] = useState<CommitDiffResult | null>(null)
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setResult(null)
    setSelectedPath(null)
    setError(null)
    try {
      setResult(await api.getGitCommitDiff(workspaceId, commitOid))
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }, [commitOid, workspaceId])

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh()
  }, [refresh])

  if (error) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 bg-editor p-4 text-center text-sm text-muted-foreground">
        <span>Failed to load commit diff: {error}</span>
        <button type="button" onClick={() => void refresh()} className="inline-flex items-center gap-1.5 rounded-md border border-border px-2 py-1 text-xs font-medium hover:bg-accent">
          <RefreshCw className="h-3.5 w-3.5" /> Retry
        </button>
      </div>
    )
  }

  if (!result) {
    return <div className="flex h-full items-center justify-center bg-editor text-sm text-muted-foreground">Loading commit diff…</div>
  }

  const selected = result.files.find((file) => file.path === selectedPath) ?? result.files[0]
  return (
    <div className="flex h-full min-h-0 flex-col bg-editor">
      <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-1.5 text-xs text-muted-foreground">
        <span className="font-medium text-foreground">Commit {result.oid.slice(0, 8)}</span>
        <span>{result.files.length} changed {result.files.length === 1 ? 'file' : 'files'}</span>
        {result.parentOid && <span className="truncate">from {result.parentOid.slice(0, 8)}</span>}
      </div>
      {result.files.length === 0 ? (
        <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">No file changes in this commit.</div>
      ) : (
        <div className="flex min-h-0 flex-1">
          <nav className="w-48 shrink-0 overflow-y-auto border-r border-border py-1" aria-label="Changed files">
            {result.files.map((file) => (
              <button
                key={file.path}
                type="button"
                onClick={() => setSelectedPath(file.path)}
                className={`block w-full truncate px-3 py-1.5 text-left text-xs hover:bg-accent ${selected?.path === file.path ? 'bg-accent text-foreground' : 'text-muted-foreground'}`}
                title={file.path}
              >
                {file.path}
              </button>
            ))}
          </nav>
          {selected && <div className="min-w-0 flex-1"><GitDiffViewer path={selected.path} base={selected.base} head={selected.head} truncated={selected.truncated} /></div>}
        </div>
      )}
    </div>
  )
}
