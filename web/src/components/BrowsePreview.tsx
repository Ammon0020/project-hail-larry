import { useEffect, useRef, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { previewFileUrl } from '@/lib/api'
import type { AppEvent } from '@/types'

/** Coalesce bursty FileWritten / FileChangedOnDisk into one iframe remount. */
const LIVE_RELOAD_DEBOUNCE_MS = 250

/**
 * Browse-preview tab — renders a multi-file static site from the workspace
 * inside a sandboxed iframe pointed at `/preview/{workspaceId}/{entryPath}`.
 * Relative asset URLs (CSS/JS/images) resolve against that path-based root.
 *
 * Live reload: when `events` includes a FileWritten / FileChangedOnDisk for
 * this workspace, the iframe remounts (key bump) after a short debounce.
 */
export function BrowsePreview({
  workspaceId,
  entryPath,
  events = [],
}: {
  workspaceId: string
  entryPath: string
  /** Shared backend event list (same WS stream as useBackend) — no extra socket. */
  events?: AppEvent[]
}) {
  // Bumping the key remounts the iframe (manual refresh + live reload).
  // Remount is more reliable than contentWindow.location.reload() under sandbox.
  const [reloadKey, setReloadKey] = useState(0)
  const src = previewFileUrl(workspaceId, entryPath)

  // Highest event id already considered — skip historical events on mount.
  const processedIdRef = useRef<number | null>(null)
  const debounceRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => {
    if (!workspaceId || events.length === 0) return
    const maxId = Math.max(...events.map((e) => e.id ?? 0))
    if (processedIdRef.current === null) {
      processedIdRef.current = maxId
      return
    }
    const since = processedIdRef.current
    if (maxId <= since) return

    // MVP: any file change in this workspace reloads the preview (CSS/JS/assets
    // may live anywhere under the root). Missing workspaceId still counts —
    // matches useBackend's tree-refresh fallback for older/partial events.
    const shouldReload = events.some(
      (e) =>
        (e.id ?? 0) > since &&
        (e.type === 'FileWritten' || e.type === 'FileChangedOnDisk') &&
        (!e.workspaceId || e.workspaceId === workspaceId),
    )
    processedIdRef.current = maxId
    if (!shouldReload) return

    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => {
      debounceRef.current = undefined
      setReloadKey((k) => k + 1)
    }, LIVE_RELOAD_DEBOUNCE_MS)
  }, [events, workspaceId])

  useEffect(
    () => () => {
      if (debounceRef.current) clearTimeout(debounceRef.current)
    },
    [],
  )

  return (
    <div className="absolute inset-0 flex flex-col bg-background">
      <div className="flex items-center gap-2 px-3 py-1.5 border-b border-border bg-panel text-xs text-muted-foreground shrink-0">
        <span className="truncate flex-1" title={entryPath}>
          Preview · {entryPath}
        </span>
        <button
          type="button"
          onClick={() => setReloadKey((k) => k + 1)}
          className="flex items-center gap-1.5 font-medium text-foreground hover:text-primary transition px-2 py-0.5 rounded"
          title="Reload preview"
        >
          <RefreshCw className="w-3.5 h-3.5" aria-hidden="true" />
          Refresh
        </button>
      </div>
      {/*
        Sandbox policy: allow-scripts + allow-same-origin so the user's own
        workspace HTML/CSS/JS can run (standard local-preview sandbox).
        allow-top-navigation stays OFF so the preview cannot redirect the IDE.
      */}
      <iframe
        key={reloadKey}
        src={src}
        title={`Preview: ${entryPath}`}
        className="flex-1 w-full border-0 bg-white"
        sandbox="allow-scripts allow-same-origin"
      />
    </div>
  )
}
