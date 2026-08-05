import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import {
  ArrowDown,
  ArrowUp,
  ChevronDown,
  Copy,
  DownloadCloud,
  Eye,
  File,
  Folder,
  GitBranch,
  GitPullRequest,
  Loader2,
  Minus,
  Plus,
  RefreshCw,
  RotateCcw,
  Send,
  MoreHorizontal,
} from 'lucide-react'
import { FileIcon } from '@/lib/fileIcon'
import { api, type FileStatus, type StatusResult } from '@/lib/api'
import { useGitState } from '@/hooks/useGitState'
import { useLongPressHandlers } from '@/hooks/useLongPressHandlers'
import { cn } from '@/lib/utils'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { BranchPicker, type BranchOption } from '@/components/git/BranchPicker'
import { GitHistorySection } from '@/components/git/GitHistorySection'

const statusStyles: Record<FileStatus['status'], { label: string; className: string }> = {
  added: { label: 'A', className: 'text-green-500' },
  modified: { label: 'M', className: 'text-amber-500' },
  deleted: { label: 'D', className: 'text-destructive' },
  renamed: { label: 'R', className: 'text-blue-500' },
  untracked: { label: 'U', className: 'text-green-500' },
  conflicted: { label: 'C', className: 'text-destructive' },
}

/** Estimated row height: py-1.5 (12px) + text-xs line (~16px) ≈ 28px. */
const ROW_HEIGHT = 28

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

function GitFileRow({
  file,
  onOpenDiff,
  onStage,
  onUnstage,
  onIgnore,
  onFileSelect,
  onDiscard,
  busy,
  menuOpen,
  onOpenMenu,
  onCloseMenu,
}: {
  file: FileStatus
  onOpenDiff: (path: string, staged: boolean) => void
  onStage: (path: string) => void
  onUnstage: (path: string) => void
  onIgnore: (path: string) => void
  onFileSelect?: (path: string) => void
  onDiscard?: (file: FileStatus) => void
  busy: boolean
  menuOpen: boolean
  onOpenMenu: () => void
  onCloseMenu: () => void
}) {
  const style = statusStyles[file.status]
  const isFolder = file.status === 'untracked' && file.path.endsWith('/')
  
  const newSegments = file.path.replace(/\/$/, '').split('/')
  const newFilename = newSegments.pop() || ''
  const newDirname = newSegments.join('/')
  
  let displayName = newFilename + (isFolder ? '/' : '')
  let displayDirname = newDirname

  if (file.oldPath) {
    const oldSegments = file.oldPath.replace(/\/$/, '').split('/')
    const oldFilename = oldSegments.pop() || ''
    const oldDirname = oldSegments.join('/')
    
    if (oldDirname === newDirname) {
      displayName = `${oldFilename} → ${newFilename}`
    } else {
      displayName = newFilename
      displayDirname = `${file.oldPath} → ${newDirname}`
    }
  }

  const stageLabel = isFolder ? `Stage folder contents: ${file.path}` : file.staged ? `Unstage ${file.path}` : `Stage ${file.path}`
  const { handlers: touchHandlers } = useLongPressHandlers(onOpenMenu)

  const row = (
    <div
      className={cn(
        'group flex items-center gap-1.5 px-3 h-7 text-xs hover:bg-accent cursor-pointer select-none',
        menuOpen && 'ring-1 ring-primary outline-none',
      )}
      title={`${file.oldPath ? `${file.oldPath} → ${file.path}` : file.path} • ${file.status}`}
      onClick={() => {
        if (!isFolder) onOpenDiff(file.path, file.staged)
      }}
      onContextMenu={(e) => {
        e.preventDefault()
        e.stopPropagation()
        onOpenMenu()
      }}
      {...touchHandlers}
    >
      <div className="flex min-w-0 flex-1 items-center gap-1.5 text-left">
        {isFolder
          ? <Folder className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          : <FileIcon name={newFilename} className="h-3.5 w-3.5 shrink-0" />}
        <div className="flex min-w-0 flex-1 items-baseline gap-1.5">
          <span className="shrink-0 truncate max-w-full text-xs">{displayName}</span>
          {displayDirname && <span className="min-w-0 truncate text-[10px] text-muted-foreground">{displayDirname}</span>}
        </div>
      </div>
      <div
        className="hidden items-center gap-0.5 group-hover:flex group-focus-within:flex"
        onClick={(e) => e.stopPropagation()}
        onMouseDown={(e) => e.stopPropagation()}
        onPointerDown={(e) => e.stopPropagation()}
      >
        {!isFolder && onFileSelect && (
          <button
            type="button"
            disabled={busy}
            onClick={(e) => {
              e.preventDefault()
              onFileSelect(file.path)
            }}
            className="shrink-0 rounded p-1 text-muted-foreground hover:bg-primary hover:text-primary-foreground transition-colors disabled:cursor-not-allowed disabled:opacity-50"
            title="Open File"
            aria-label={`Open ${file.path}`}
          >
            <File className="h-3.5 w-3.5" />
          </button>
        )}
        {!file.staged && onDiscard && (
          <button
            type="button"
            disabled={busy}
            onClick={(e) => {
              e.preventDefault()
              onDiscard(file)
            }}
            className="shrink-0 rounded p-1 text-muted-foreground hover:bg-destructive hover:text-destructive-foreground transition-colors disabled:cursor-not-allowed disabled:opacity-50"
            title="Discard Changes"
            aria-label={`Discard changes in ${file.path}`}
          >
            <RotateCcw className="h-3.5 w-3.5" />
          </button>
        )}
        <button
          type="button"
          disabled={busy}
          onClick={(e) => {
            e.preventDefault()
            if (file.staged) {
              onUnstage(file.path)
            } else {
              onStage(file.path)
            }
          }}
          className="shrink-0 rounded p-1 text-muted-foreground hover:bg-primary hover:text-primary-foreground transition-colors disabled:cursor-not-allowed disabled:opacity-50"
          aria-label={stageLabel}
          title={stageLabel}
        >
          {file.staged ? <Minus className="h-3.5 w-3.5" /> : <Plus className="h-3.5 w-3.5" />}
        </button>
      </div>
      <span className={cn('shrink-0 font-semibold text-xs ml-1', style.className)}>{style.label}</span>
    </div>
  )

  return (
    <DropdownMenu
      open={menuOpen}
      onOpenChange={(open) => {
        if (!open) onCloseMenu()
      }}
    >
      <DropdownMenuTrigger asChild>{row}</DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {!isFolder && (
          <DropdownMenuItem
            onSelect={() => {
              onOpenDiff(file.path, file.staged)
              onCloseMenu()
            }}
          >
            <Eye className="w-3.5 h-3.5" />
            Open Diff
          </DropdownMenuItem>
        )}
        <DropdownMenuItem
          onSelect={() => {
            if (file.staged) onUnstage(file.path)
            else onStage(file.path)
            onCloseMenu()
          }}
        >
          {file.staged
            ? <Minus className="w-3.5 h-3.5" />
            : <Plus className="w-3.5 h-3.5" />}
          {file.staged ? 'Unstage' : 'Stage'}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={() => {
            onIgnore(file.path)
            onCloseMenu()
          }}
        >
          <Folder className="w-3.5 h-3.5" />
          {isFolder ? 'Add folder to .gitignore' : 'Add to .gitignore'}
        </DropdownMenuItem>
        <DropdownMenuItem
          onSelect={() => {
            void navigator.clipboard.writeText(file.path)
            onCloseMenu()
          }}
        >
          <Copy className="w-3.5 h-3.5" />
          Copy Path
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
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
  onIgnore,
  onFileSelect,
  onDiscard,
  busy,
  scrollRef,
  menuPath,
  setMenuPath,
}: {
  title: string
  files: FileStatus[]
  staged: boolean
  hint?: string
  onStage: (paths: string[], all: boolean) => void
  onUnstage: (paths: string[]) => void
  onOpenDiff: (path: string, staged: boolean) => void
  onIgnore: (path: string) => void
  onFileSelect?: (path: string) => void
  onDiscard?: (file: FileStatus) => void
  busy: boolean
  scrollRef: React.RefObject<HTMLDivElement | null>
  menuPath: string | null
  setMenuPath: (path: string | null) => void
}) {
  // Virtualize the file rows so a workspace with thousands of untracked
  // entries (e.g. `target/` expanded by `--untracked-files=all`) only mounts
  // the visible ones. The section header + hint render above the virtualized
  // list and scroll with it because they live in the same scroll parent.
  // The hook must run on every render (no early return before it) to satisfy
  // the rules-of-hooks; the empty-list short-circuit happens after.
  // TanStack Virtual's useVirtualizer returns functions that the React Compiler
  // cannot memoize safely (it would close over stale scroll/size state); the
  // hook is called unconditionally here and its values stay local to this
  // component, so skipping compiler memoization is intentional and safe.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: files.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
  })

  if (files.length === 0) return null
  const actionLabel = staged ? 'Unstage All Changes' : 'Stage All Changes'
  const items = virtualizer.getVirtualItems()

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
      <div style={{ position: 'relative', height: virtualizer.getTotalSize() }}>
        {items.map((vi) => {
          const file = files[vi.index]
          return (
            <div
              key={`${file.staged}:${file.path}`}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                transform: `translateY(${vi.start}px)`,
              }}
            >
              <GitFileRow
                file={file}
                onOpenDiff={onOpenDiff}
                onStage={(path) => onStage([path], false)}
                onUnstage={(path) => onUnstage([path])}
                onIgnore={onIgnore}
                onFileSelect={onFileSelect}
                onDiscard={onDiscard}
                busy={busy}
                menuOpen={menuPath === file.path}
                onOpenMenu={() => setMenuPath(file.path)}
                onCloseMenu={() => setMenuPath(null)}
              />
            </div>
          )
        })}
      </div>
    </section>
  )
}

/** Source Control sidebar with repository initialization, staging, commits, and push. */
export function GitPanel({
  workspaceId,
  onOpenDiff,
  onOpenCommitDiff,
  onRepoChanged,
  onFileSelect,
}: {
  workspaceId: string | null
  onOpenDiff: (path: string, staged: boolean) => void
  onOpenCommitDiff: (commitOid: string) => void
  onRepoChanged: () => Promise<void>
  onFileSelect?: (path: string) => void
}) {
  const { gitState, loading: gitStateLoading, refresh: refreshGitState } = useGitState(workspaceId)
  const [status, setStatus] = useState<StatusResult | null>(null)
  const [message, setMessage] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busyAction, setBusyAction] = useState<string | null>(null)
  const [historyRefresh, setHistoryRefresh] = useState(0)
  // Single scroll parent for both sections; each ChangeSection's virtualizer
  // reads this ref as its scroll element. One shared scroll keeps the UX
  // (Staged + Changes scroll together) and avoids two competing containers.
  const scrollRef = useRef<HTMLDivElement>(null)
  // Controlled context-menu state: the path of the row whose menu is open, or
  // null. Shared by both sections so only one menu is open at a time.
  const [menuPath, setMenuPath] = useState<string | null>(null)
  const [branchPickerOpen, setBranchPickerOpen] = useState(false)

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
      setHistoryRefresh((value) => value + 1)
    } catch (err) {
      setError(errorMessage(err))
      if (action === 'commit') await refreshStatus()
    } finally {
      setBusyAction(null)
    }
  }, [refreshStatus])

  const stagedFiles = useMemo(() => status?.files.filter((file) => file.staged) ?? [], [status])
  const unstagedFiles = useMemo(() => status?.files.filter((file) => !file.staged) ?? [], [status])
  // Dedup local + remote branches. Local branches take precedence; remote-only
  // branches keep their full `origin/branch` display but checkout uses the
  // short name so git auto-creates a tracking branch.
  const branchOptions = useMemo(() => {
    const raw = status?.branches ?? []
    const seen = new Set<string>()
    const result: BranchOption[] = []
    for (const b of raw) {
      if (!b.includes('/')) {
        seen.add(b)
        result.push({ name: b, display: b, isRemote: false })
      }
    }
    for (const b of raw) {
      const slash = b.indexOf('/')
      if (slash < 0) continue
      const short = b.slice(slash + 1)
      if (!seen.has(short)) {
        seen.add(short)
        result.push({ name: short, display: b, isRemote: true })
      }
    }
    return result
  }, [status?.branches])
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
      <BranchPicker
        open={branchPickerOpen}
        onOpenChange={setBranchPickerOpen}
        branches={branchOptions}
        currentBranch={status?.headBranch ?? gitState.headBranch}
        busy={busy}
        onCheckout={(branch) => {
          void runMutation('checkout', async () => {
            await api.gitCheckout(workspaceId, branch)
            await onRepoChanged()
          })
        }}
      />
      <header className="flex items-center justify-between border-b border-border px-3 py-2">
        <div className="min-w-0">
          <button
            type="button"
            disabled={busy}
            onClick={() => setBranchPickerOpen(true)}
            className="flex items-center gap-1.5 text-xs font-medium rounded px-1 -mx-1 hover:bg-secondary disabled:cursor-not-allowed disabled:opacity-50 outline-none"
          >
            <GitBranch className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            <span className={cn('truncate', !status?.headBranch && !gitState.headBranch && 'text-amber-500')}>
              {status?.headBranch ?? gitState.headBranch ?? 'Detached HEAD'}
            </span>
            {branchOptions.length > 1 && <ChevronDown className="h-3 w-3 shrink-0 text-muted-foreground" />}
          </button>
          {status && (status.upstream || status.ahead || status.behind) && (
            <div className="mt-0.5 flex items-center gap-1 text-[10px] text-muted-foreground">
              {status.upstream && <span className="truncate">{status.upstream}</span>}
              {status.ahead > 0 && <span className="flex items-center"><ArrowUp className="h-2.5 w-2.5" />{status.ahead}</span>}
              {status.behind > 0 && <span className="flex items-center"><ArrowDown className="h-2.5 w-2.5" />{status.behind}</span>}
            </div>
          )}
        </div>
        <div className="flex items-center gap-1 shrink-0">
          <button type="button" disabled={busy} onClick={() => void runMutation('refresh', async () => { /* refreshStatus runs as runMutation's trailing reload */ })} className="rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50" aria-label="Refresh source control" title="Refresh">
            <RefreshCw className={cn('h-3.5 w-3.5', busyAction === 'refresh' && 'animate-spin')} />
          </button>
          <button type="button" disabled={busy} onClick={() => void runMutation('fetch', async () => { await api.gitFetch(workspaceId) })} className="rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50" aria-label="Fetch from remote" title="Fetch">
            {busyAction === 'fetch' ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <DownloadCloud className="h-3.5 w-3.5" />}
          </button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button type="button" className="rounded p-1 text-muted-foreground hover:bg-secondary hover:text-foreground outline-none" aria-label="More actions" title="More actions">
                <MoreHorizontal className="h-3.5 w-3.5" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem
                disabled={busy}
                onSelect={() => {
                  void runMutation('pull', async () => {
                    await api.gitPull(workspaceId)
                    await onRepoChanged()
                  })
                }}
              >
                <GitPullRequest className="w-3.5 h-3.5" />
                Pull from remote
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
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

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto pb-2">
        <ChangeSection title="Staged Changes" files={stagedFiles} staged onStage={(paths, all) => void runMutation('stage', async () => { await api.gitStage(workspaceId, paths, all) })} onUnstage={(paths) => void runMutation('unstage', async () => { await api.gitUnstage(workspaceId, paths) })} onOpenDiff={onOpenDiff} onIgnore={(path) => void runMutation('ignore', async () => { await api.gitIgnore(workspaceId, [path]) })} onFileSelect={onFileSelect} busy={busy} scrollRef={scrollRef} menuPath={menuPath} setMenuPath={setMenuPath} />
        <ChangeSection
          title="Changes"
          files={unstagedFiles}
          staged={false}
          hint={allUntrackedHint}
          onStage={(paths, all) => void runMutation('stage', async () => { await api.gitStage(workspaceId, paths, all) })}
          onUnstage={(paths) => void runMutation('unstage', async () => { await api.gitUnstage(workspaceId, paths) })}
          onOpenDiff={onOpenDiff}
          onIgnore={(path) => void runMutation('ignore', async () => { await api.gitIgnore(workspaceId, [path]) })}
          onFileSelect={onFileSelect}
          onDiscard={(file) => {
            if (!workspaceId) return
            const label = file.status === 'untracked'
              ? `Delete untracked file "${file.path}"? This cannot be undone.`
              : `Discard changes to "${file.path}"? This cannot be undone.`
            if (!window.confirm(label)) return
            void runMutation('discard', async () => {
              await api.gitDiscard(workspaceId, [file.path])
            })
          }}
          busy={busy}
          scrollRef={scrollRef}
          menuPath={menuPath}
          setMenuPath={setMenuPath}
        />
        {status && status.files.length === 0 && <div className="p-6 text-center text-xs text-muted-foreground">No changes.</div>}
      </div>
      <GitHistorySection workspaceId={workspaceId} onOpenCommitDiff={onOpenCommitDiff} refreshKey={historyRefresh} />
    </div>
  )
}
