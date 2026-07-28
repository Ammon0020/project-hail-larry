import { useEffect, useRef, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { api, previewFileUrl } from '@/lib/api'
import { TrustPrompt } from '@/components/preview/TrustPrompt'
import type { AppEvent } from '@/types'

/** Coalesce bursty FileWritten / FileChangedOnDisk into one iframe remount. */
const LIVE_RELOAD_DEBOUNCE_MS = 250
const PREVIEW_AUTH_ROUTE_UNAVAILABLE =
  'Preview authorization needs a server restart to finish updating.'

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
  trusted,
}: {
  workspaceId: string
  entryPath: string
  /** Shared backend event list (same WS stream as useBackend) — no extra socket. */
  events?: AppEvent[]
  /** Per-workspace HTML preview trust state. Unknown (`null`/`undefined`)
   *  shows a trust prompt before rendering the iframe; `true`/`false` skip
   *  it and render immediately with the backend's CSP. */
  trusted?: boolean | null
}) {
  // A fresh session remounts the sandboxed iframe for manual and live reloads.
  const [previewSessionVersion, setPreviewSessionVersion] = useState(0)
  const reloadPreview = () => setPreviewSessionVersion((version) => version + 1)
  const [previewToken, setPreviewToken] = useState<string>()
  const [sessionError, setSessionError] = useState<string>()
  // Local override once the user answers the trust prompt — avoids waiting for
  // a parent re-render before showing the iframe.
  const [resolvedTrust, setResolvedTrust] = useState<boolean | null | undefined>(undefined)
  const src = previewToken ? previewFileUrl(workspaceId, entryPath, previewToken) : ''
  // Effective trust: a local override (from answering the prompt) takes
  // precedence over the prop so the iframe renders without waiting for the
  // parent to re-render after setWorkspaceTrust updates backend state.
  const effectiveTrust = resolvedTrust ?? trusted
  const trustUnknown = effectiveTrust == null

  // Reset stale token/error when the workspace or session version changes,
  // during render (React's "adjust state on prop change" pattern) so the
  // iframe doesn't briefly show the old workspace's content.
  const [prevResetKey, setPrevResetKey] = useState(`${workspaceId}:${previewSessionVersion}`)
  const resetKey = `${workspaceId}:${previewSessionVersion}`
  if (prevResetKey !== resetKey) {
    setPrevResetKey(resetKey)
    setPreviewToken(undefined)
    setSessionError(undefined)
    setResolvedTrust(undefined)
  }

  useEffect(() => {
    let cancelled = false
    if (!workspaceId) return
    void api.createPreviewSession(workspaceId)
      .then(({ token }) => {
        if (!cancelled) setPreviewToken(token)
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setSessionError(
            error instanceof Error && error.message === 'Method Not Allowed'
              ? PREVIEW_AUTH_ROUTE_UNAVAILABLE
              : error instanceof Error
                ? error.message
                : 'Unable to authorize preview',
          )
        }
      })
    return () => { cancelled = true }
  }, [workspaceId, previewSessionVersion])

  // Highest event id already considered — skip historical events on mount.
  const processedIdRef = useRef<number | null>(null)
  const debounceRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  useEffect(() => {
    if (!workspaceId || events.length === 0) return
    // reduce instead of Math.max(...spread) — large replay lists can blow the
    // call stack when spread into Math.max.
    const maxId = events.reduce((m, e) => Math.max(m, e.id ?? 0), 0)
    if (processedIdRef.current === null) {
      processedIdRef.current = maxId
      return
    }
    const since = processedIdRef.current
    if (maxId <= since) return

    // Only reload when the written file is part of the previewed site: the
    // entry's directory (root if the entry is at the workspace root) contains
    // the target path. Falls back to "any write" when the entry path can't be
    // resolved to a directory.
    const entryDir = entryPath.includes('/') ? entryPath.slice(0, entryPath.lastIndexOf('/') + 1) : ''
    const isRelevant = (e: AppEvent) =>
      !entryDir || (e.target ?? '').startsWith(entryDir)
    const shouldReload = events.some(
      (e) =>
        (e.id ?? 0) > since &&
        (e.type === 'FileWritten' || e.type === 'FileChangedOnDisk') &&
        (!e.workspaceId || e.workspaceId === workspaceId) &&
        isRelevant(e),
    )
    processedIdRef.current = maxId
    if (!shouldReload) return

    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => {
      debounceRef.current = undefined
      reloadPreview()
    }, LIVE_RELOAD_DEBOUNCE_MS)
  }, [events, workspaceId, entryPath])

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
          onClick={reloadPreview}
          className="flex items-center gap-1.5 font-medium text-foreground hover:text-primary transition px-2 py-0.5 rounded"
          title="Reload preview"
        >
          <RefreshCw className="w-3.5 h-3.5" aria-hidden="true" />
          Refresh
        </button>
      </div>
      {/*
        Sandbox: allow-scripts only (no allow-same-origin). The iframe gets an
        opaque origin so workspace HTML/JS cannot read the IDE's localStorage or
        call authenticated APIs as the parent app. Relative CSS/JS/image URLs
        use the HttpOnly, path-scoped preview cookie.
        allow-top-navigation stays OFF so the preview cannot redirect the IDE.
      */}
      {sessionError ? (
        <div className="flex-1 flex flex-col items-center justify-center gap-3 px-6 text-center text-sm text-destructive">
          <p>Preview authorization failed: {sessionError}</p>
          <button
            type="button"
            onClick={reloadPreview}
            className="rounded px-2 py-1 font-medium text-foreground hover:text-primary"
          >
            Retry preview
          </button>
        </div>
      ) : trustUnknown ? (
        <TrustPrompt
          workspaceId={workspaceId}
          onResolve={setResolvedTrust}
          className="flex-1 text-destructive"
        />
      ) : effectiveTrust === false ? (
        <div className="flex-1 flex flex-col items-center justify-center gap-2 px-6 text-center text-sm text-muted-foreground">
          <p>Preview blocked — mark as trusted to view.</p>
        </div>
      ) : previewToken ? (
        <iframe
          src={src}
          title={`Preview: ${entryPath}`}
          className="flex-1 w-full border-0 bg-white"
          sandbox="allow-scripts"
        />
      ) : (
        <div className="flex-1 flex items-center justify-center text-sm text-muted-foreground">
          Authorizing preview…
        </div>
      )}
    </div>
  )
}
