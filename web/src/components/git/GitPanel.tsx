import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  ArrowDown,
  ArrowUp,
  Circle,
  CircleDot,
  Folder,
  GitBranch,
  Loader2,
  Minus,
  Plus,
  RefreshCw,
  Send,
} from 'lucide-react'
import { api, type FileStatus, type StatusResult } from '@/lib/api'
import { useGitState } from '@/hooks/useGitState'
import { cn } from '@/lib/utils'

const statusStyles: Record<FileStatus['status'], { label: string; className: string }> = {
  added: { label: 'A', className: 'text-green-500' },
  modified: { label: 'M', className: 'text-amber-500' },
  deleted: { label: 'D', className: 'text-destructive' },
  renamed: { label: 'R', className: 'text-blue-500' },
  untracked: { label: 'U', className: 'text-green-500' },
  conflicted: { label: 'C', className: 'text-destructive' },
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

function GitFileRow({
  file,
  onOpenDiff,
  onStage,
  onUnstage,
  busy,
}: {
  file: FileStatus
  onOpenDiff: (path: string, staged: boolean) => void
  onStage: (path: string) => void
  onUnstage: (path: string) => void
  busy: boolean
}) {
  const style = statusStyles[file.status]
  const isFolder = file.status === 'untracked' && file.path.endsWith('/')
  const displayPath = file.oldPath ? `${file.oldPath} → ${file.path}` : file.path
  const stageLabel = isFolder ? `Stage folder contents: ${file.path}` : file.staged ? `Unstage ${file.path}` : `Stage ${file.path}`
  return (
    <div
      className="group flex items-center gap-2 px-3 py-1.5 text-xs hover:bg-accent"
      title={displayPath}
    >
      <button
        type="button"
        className="flex min-w-0 flex-1 items-center gap-2 text-left"
        onClick={() => !isFolder && onOpenDiff(file.path, file.staged)}
      >
        {isFolder
          ? <Folder className={cn('h-3.5 w-3.5 shrink-0', style.className)} />
          : file.status === 'untracked'
            ? <Circle className={cn('h-3.5 w-3.5 shrink-0', style.className)} />
            : <CircleDot className={cn('h-3.5 w-3.5 shrink-0', style.className)} />}
        <span className="min-w-0 flex-1 truncate">{displayPath}</span>
        <span className={cn('shrink-0 font-semibold', style.className)}>{style.label}</span>
      </button>
      <button
        type="button"
        disabled={busy}
        onClick={() => file.staged ? onUnstage(file.path) : onStage(file.path)}
        className="shrink-0 rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
        aria-label={stageLabel}
        title={stageLabel}
      >
        {file.staged ? <Minus className="h-3.5 w-3.5" /> : <Plus className="h-3.5 w-3.5" />}
      </button>
    </div>
  )
}

function ChangeSection({
  title,
  files,
  staged,
  hint,
  onStage,
  onUnstage,
  onOpenDiff,
  busy,
}: {
  title: string
  files: FileStatus[]
  staged: boolean
  hint?: string
  onStage: (paths: string[], all: boolean) => void
  onUnstage: (paths: string[]) => void
  onOpenDiff: (path: string, staged: boolean) => void
  busy: boolean
}) {
  if (files.length === 0) return null
  const actionLabel = staged ? 'Unstage All Changes' : 'Stage All Changes'
  return (
    <section>
      <div className="flex items-center justify-between px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        <span>{title} ({files.length})</span>
        {staged ? (
          <button type="button" disabled={busy} onClick={() => onUnstage(files.map((file) => file.path))} className="rounded p-1 hover:bg-secondary hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50" aria-label={actionLabel} title={actionLabel}>
            <Minus className="h-3.5 w-3.5" />
          </button>
        ) : (
          <button type="button" disabled={busy} onClick={() => onStage([], true)} className="rounded px-1.5 py-0.5 text-[10px] font-medium normal-case tracking-normal text-primary hover:bg-secondary disabled:cursor-not-allowed disabled:opacity-50" aria-label={actionLabel} title={actionLabel}>
            Stage All
          </button>
        )}
      </div>
      {hint && <div className="px-3 pb-1.5 text-[10px] text-muted-foreground">{hint}</div>}
      {files.map((file) => (
        <GitFileRow
          key={`${file.staged}:${file.path}`}
          file={file}
          onOpenDiff={onOpenDiff}
          onStage={(path) => onStage([path], false)}
          onUnstage={(path) => onUnstage([path])}
          busy={busy}
        />
      ))}
    </section>
  )
}

/** Source Control sidebar with repository initialization, staging, commits, and push. */
export function GitPanel({
  workspaceId,
  onOpenDiff,
  onRepoChanged,
}: {
  workspaceId: string | null
  onOpenDiff: (path: string, staged: boolean) => void
  onRepoChanged: () => Promise<void>
}) {
  const { gitState, loading: gitStateLoading, refresh: refreshGitState } = useGitState(workspaceId)
  const [status, setStatus] = useState<StatusResult | null>(null)
  const [message, setMessage] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busyAction, setBusyAction] = useState<string | null>(null)

  const refreshStatus = useCallback(async () => {
    if (!workspaceId || !gitState?.repoDetected) {
      setStatus(null)
      return
    }
    setError(null)
    try {
      setStatus(await api.getGitStatus(workspaceId))
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [gitState?.repoDetected, workspaceId])

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refreshStatus()
  }, [refreshStatus])

  const runMutation = useCallback(async (action: string, mutation: () => Promise<void>) => {
    setBusyAction(action)
    setError(null)
    try {
      await mutation()
      await refreshStatus()
    } catch (err) {
      setError(errorMessage(err))
      if (action === 'commit') await refreshStatus()
    } finally {
      setBusyAction(null)
    }
  }, [refreshStatus])

  const stagedFiles = useMemo(() => status?.files.filter((file) => file.staged) ?? [], [status])
  const unstagedFiles = useMemo(() => status?.files.filter((file) => !file.staged) ?? [], [status])
  const allUntrackedHint = stagedFiles.length === 0 && unstagedFiles.length > 0 && unstagedFiles.every((file) => file.status === 'untracked') ? 'New files — stage them, then commit.' : undefined
  const canCommit = !!message.trim() && stagedFiles.length > 0
  const busy = busyAction !== null

  const commit = useCallback(() => {
    if (!canCommit || busy || !workspaceId) return
    void runMutation('commit', async () => {
      await api.gitCommit(workspaceId, message.trim(), false, status?.headOid ?? null)
      setMessage('')
      await onRepoChanged()
    })
  }, [busy, canCommit, message, onRepoChanged, runMutation, status?.headOid, workspaceId])

  if (!workspaceId) {
    return <div className="p-6 text-center text-sm text-muted-foreground">Select a workspace to use source control.</div>
  }

  if (gitStateLoading) {
    return <div className="flex items-center justify-center gap-2 p-6 text-sm text-muted-foreground"><Loader2 className="h-4 w-4 animate-spin" /> Loading source control…</div>
  }

  if (!gitState?.repoDetected) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
        <GitBranch className="h-8 w-8 text-muted-foreground" />
        <div>
          <p className="text-sm font-medium">No Git repository detected</p>
          <p className="mt-1 text-xs text-muted-foreground">Initialize this workspace to start tracking changes.</p>
        </div>
        <button
          type="button"
          disabled={busy}
          onClick={() => void runMutation('init', async () => {
            await api.gitInit(workspaceId)
            await refreshGitState()
            await onRepoChanged()
          })}
          className="rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busyAction === 'init' ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : 'Initialize Repository'}
        </button>
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-border px-3 py-2">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-xs font-medium">
            <GitBranch className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            <span className="truncate">{status?.headBranch ?? gitState.headBranch ?? 'Detached HEAD'}</span>
          </div>
          {status && (status.upstream || status.ahead || status.behind) && (
            <div className="mt-0.5 flex items-center gap-1 text-[10px] text-muted-foreground">
              {status.upstream && <span className="truncate">{status.upstream}</span>}
              {status.ahead > 0 && <span className="flex items-center"><ArrowUp className="h-2.5 w-2.5" />{status.ahead}</span>}
              {status.behind > 0 && <span className="flex items-center"><ArrowDown className="h-2.5 w-2.5" />{status.behind}</span>}
            </div>
          )}
        </div>
        <button type="button" disabled={busy} onClick={() => void runMutation('refresh', async () => { /* refreshStatus runs as runMutation's trailing reload */ })} className="rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50" aria-label="Refresh source control" title="Refresh">
          <RefreshCw className={cn('h-3.5 w-3.5', busyAction === 'refresh' && 'animate-spin')} />
        </button>
      </header>

      <div className="border-b border-border p-3">
        <label htmlFor="git-commit-message" className="sr-only">Commit message</label>
        <textarea
          id="git-commit-message"
          value={message}
          onChange={(event) => setMessage(event.target.value)}
          onKeyDown={(event) => {
            if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
              event.preventDefault()
              commit()
            }
          }}
          placeholder="Message (press Ctrl+Enter to commit)"
          rows={3}
          className="w-full resize-none rounded-md border border-input bg-background px-2 py-1.5 text-xs outline-none transition focus:border-ring"
        />
        <div className="mt-2 flex gap-2">
          <button type="button" disabled={!canCommit || busy} onClick={commit} className="flex flex-1 items-center justify-center gap-1.5 rounded-md bg-primary px-2 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50">
            {busyAction === 'commit' && <Loader2 className="h-3.5 w-3.5 animate-spin" />} Commit
          </button>
          <button type="button" disabled={busy} onClick={() => void runMutation('push', async () => { await api.gitPush(workspaceId) })} className="flex items-center justify-center gap-1.5 rounded-md border border-border px-2 py-1.5 text-xs font-medium hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50">
            {busyAction === 'push' ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Send className="h-3.5 w-3.5" />} Push
          </button>
        </div>
      </div>

      {error && <div className="mx-3 mt-3 rounded-md border border-destructive/30 bg-destructive/10 px-2 py-1.5 text-xs text-destructive">{error}</div>}

      <div className="flex-1 overflow-y-auto pb-2">
        <ChangeSection title="Staged Changes" files={stagedFiles} staged onStage={(paths, all) => void runMutation('stage', async () => { await api.gitStage(workspaceId, paths, all) })} onUnstage={(paths) => void runMutation('unstage', async () => { await api.gitUnstage(workspaceId, paths) })} onOpenDiff={onOpenDiff} busy={busy} />
        <ChangeSection title="Changes" files={unstagedFiles} staged={false} hint={allUntrackedHint} onStage={(paths, all) => void runMutation('stage', async () => { await api.gitStage(workspaceId, paths, all) })} onUnstage={(paths) => void runMutation('unstage', async () => { await api.gitUnstage(workspaceId, paths) })} onOpenDiff={onOpenDiff} busy={busy} />
        {status && status.files.length === 0 && <div className="p-6 text-center text-xs text-muted-foreground">No changes.</div>}
      </div>
    </div>
  )
}
