import { useCallback, useEffect, useRef, useState } from 'react'
import { api, getDeviceCredential, type AppEvent, type WorkspaceInfo, type FileNode, type DeviceCredential, type PendingPermission } from '@/lib/api'
import type { Agent, Session } from '@/types'
import { useFileActions } from '@/hooks/useFileActions'
import { useSessionActions } from '@/hooks/useSessionActions'
import { safeStorage } from '@/lib/safeStorage'

/**
 * Upper bound on how many events we keep in memory at once. The in-memory
 * event log is a cache, not the source of truth: the backend SQLite event
 * store is append-only and authoritative, and loadSessionEvents(sessionId)
 * re-fetches a session's events on demand. That makes evicting the oldest
 * in-memory events SAFE — re-selecting a session repopulates its events from
 * SQLite — while preventing unbounded memory growth on long-lived sessions
 * where WebSocket events would otherwise accumulate forever.
 */
const MAX_EVENTS = 5000
const EVENT_PAGE_SIZE = 1000

/** Durable IDs are monotonic, so sorting restores chronological rendering after a merge. */
function byEventId(a: AppEvent, b: AppEvent) {
  return (a.id ?? 0) - (b.id ?? 0)
}

/**
 * useBackend — real backend hook that connects to the Go daemon.
 *
 * Manages:
 * - WebSocket connection for real-time event streaming (Blueprint Sec 12)
 * - REST API calls for workspaces, agents, sessions, pairing
 * - Event state derived from the event log
 */
export function useBackend() {
  const [workspaces, setWorkspaces] = useState<WorkspaceInfo[]>([])
  const [activeWorkspace, setActiveWorkspace] = useState<WorkspaceInfo | null>(null)
  const [fileTree, setFileTree] = useState<FileNode[]>([])
  const [agents, setAgents] = useState<Agent[]>([])
  const [sessions, setSessions] = useState<Session[]>([])
  const [events, setEvents] = useState<AppEvent[]>([])
  const [hasOlderSessionEvents, setHasOlderSessionEvents] = useState(false)
  const [devices, setDevices] = useState<DeviceCredential[]>([])
  const [pendingPermissions, setPendingPermissions] = useState<PendingPermission[]>([])
  const [connected, setConnected] = useState(false)
  // reconnecting is true only while the WebSocket is closed AND retrying after a
  // prior successful connection. It stays false on the initial connect attempt
  // (before the first onopen) so the UI does not flash a "Reconnecting…" banner
  // on a cold load. lastConnectedAt records the timestamp of the last successful
  // ws.onopen and gates the reconnecting flag.
  const [reconnecting, setReconnecting] = useState(false)
  // reconnectFailed flips to true after the socket has been down for a while
  // (RECONNECT_GIVEUP_MS) so the banner can switch from "Reconnecting…" to a
  // fatal "Connection lost — click to retry" state instead of pulsing forever.
  const [reconnectFailed, setReconnectFailed] = useState(false)
  // Multi-client session-list sync queues (Blueprint Sec 12). WS events append
  // ids; ChatPanel drains them via consumeSessionCreated/Closed so back-to-back
  // creates/closes are not last-write-wins and sticky signals cannot loop.
  const [pendingCreatedSessionIds, setPendingCreatedSessionIds] = useState<string[]>([])
  const [pendingClosedSessionIds, setPendingClosedSessionIds] = useState<string[]>([])
  const lastConnectedAtRef = useRef<number | null>(null)

  const wsRef = useRef<WebSocket | null>(null)
  const eventsRef = useRef<AppEvent[]>([])
  // Mirror activeWorkspace in a ref so the WebSocket onmessage closure (created
  // once at mount) can read the current value instead of a stale snapshot.
  const activeWorkspaceRef = useRef<WorkspaceInfo | null>(null)
  // Tracks whether the hook is still mounted so the onclose reconnect path
  // does not schedule a new WebSocket against an unmounted hook (memory leak).
  const mountedRef = useRef(true)
  // Holds the pending reconnect timer so it can be cleared on unmount.
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  // After this many ms of continuous reconnecting, the banner switches to the
  // fatal "Connection lost — click to retry" state. Long enough that transient
  // Wi-Fi blips don't trip it, short enough that a dead daemon is surfaced.
  const RECONNECT_GIVEUP_MS = 60_000
  // Timer that arms reconnectFailed; cleared on a successful onopen.
  const reconnectGiveupTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  // Debounces file-tree refreshes triggered by FileWritten / FileChangedOnDisk
  // bursts (bulk agent edits, rapid on-disk saves) so we hit getFileTree once.
  const treeRefreshTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  // Reconnect attempt counter for exponential backoff. Reset to 0 on every
  // successful ws.onopen so a long-lived connection doesn't ramp the delay.
  const reconnectAttemptRef = useRef(0)

  /** Coalesce rapid file events into one tree reload (~200ms after the last). */
  function scheduleFileTreeRefresh() {
    if (treeRefreshTimerRef.current) clearTimeout(treeRefreshTimerRef.current)
    treeRefreshTimerRef.current = setTimeout(() => {
      treeRefreshTimerRef.current = undefined
      void refreshFileTreeRef.current()
    }, 200)
  }

  /**
   * Computes the next reconnect delay using exponential backoff (base 1s,
   * doubling per attempt, capped at 30s) with ±20% jitter so a fleet of
   * clients that all dropped at the same instant don't synchronously hammer
   * the backend on retry (thundering herd).
   *
   * Args:
   *   attempt: The current (pre-increment) reconnect attempt number.
   *
   * Returns:
   *   The delay in milliseconds to wait before the next connectWebSocket call.
   */
  function nextReconnectDelay(attempt: number): number {
    const base = Math.min(30000, 1000 * 2 ** attempt)
    const jitter = base * 0.2 * (Math.random() * 2 - 1) // ±20%
    return Math.max(0, Math.round(base + jitter))
  }

  /**
   * Schedules a connectWebSocket call after `delay` ms, clearing any prior
   * pending timer first so overlapping disconnects don't stack multiple
   * sockets. No-op if the hook is already unmounted.
   */
  function scheduleReconnect(delay: number) {
    if (!mountedRef.current) return
    if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current)
    reconnectTimerRef.current = setTimeout(() => connectWebSocket(), delay)
  }

  /**
   * Immediate reconnect entry point used by the 'online' and
   * 'visibilitychange' handlers: clears any pending backoff timer, resets the
   * attempt counter (the network situation changed, so the backoff ramp no
   * longer applies), and calls connectWebSocket right away. No-op if the hook
   * is unmounted or the socket is already OPEN.
   */
  function reconnectNow() {
    if (!mountedRef.current) return
    const ws = wsRef.current
    if (ws && ws.readyState === WebSocket.OPEN) return
    if (reconnectTimerRef.current) {
      clearTimeout(reconnectTimerRef.current)
      reconnectTimerRef.current = undefined
    }
    // Manual retry clears the fatal banner and re-arms the polite pulsing one.
    setReconnectFailed(false)
    reconnectAttemptRef.current = 0
    connectWebSocket()
  }

  // Stable listeners for network/tab-return triggers. Defined inline so they
  // can be registered and torn down in the mount effect below.
  function onOnline() {
    reconnectNow()
  }
  function onVisibility() {
    // Only act when the tab returns to the foreground — a hidden tab shouldn't
    // churn reconnect attempts (browsers throttle timers anyway).
    if (document.visibilityState === 'visible') reconnectNow()
  }
  useEffect(() => {
    activeWorkspaceRef.current = activeWorkspace
  }, [activeWorkspace])

  /**
   * Commits a new event list to both the ref and React state, enforcing the
   * MAX_EVENTS cap. When the list exceeds the cap the oldest events (front of
   * the array) are evicted so only the newest MAX_EVENTS survive — events are
   * appended in arrival/ID order, so the tail is the newest. Every mutation of
   * the event log routes through here so the cap is applied uniformly and the
   * unbounded-growth problem cannot reappear at any call site. Eviction is
   * safe: SQLite is the source of truth and loadSessionEvents re-fetches a
   * session's events on demand (see MAX_EVENTS).
   */
  const commitEvents = useCallback((next: AppEvent[] | ((prev: AppEvent[]) => AppEvent[])) => {
    // Dedupe by event id as a safety net. The WS onmessage handler and REST
    // load* functions pre-filter duplicates for performance (avoiding needless
    // setEvents calls / re-renders), but centralizing the check here guarantees
    // correctness even if a caller forgets to pre-filter — duplicate ToolStarted
    // events with the same toolCallId would otherwise crash assistant-ui's
    // useResources reconciler ("Duplicate key"). Events without an id (mock
    // data, merged stream events) are always kept. Uses a local Set so there
    // are no ref mutations inside React's updater (StrictMode-safe).
    const dedupe = (arr: AppEvent[]): AppEvent[] => {
      const seen = new Set<number>()
      const result: AppEvent[] = []
      for (const e of arr) {
        if (e.id === undefined || !seen.has(e.id)) {
          if (e.id !== undefined) seen.add(e.id)
          result.push(e)
        }
      }
      return result
    }
    if (typeof next === 'function') {
      // Functional path: build the new list once inside React's updater so the
      // hot WS onmessage path doesn't spread the whole event log per token.
      setEvents((prev) => {
        const built = next(prev)
        const trimmed = built.length > MAX_EVENTS ? built.slice(built.length - MAX_EVENTS) : built
        const deduped = dedupe(trimmed)
        eventsRef.current = deduped
        return deduped
      })
    } else {
      const trimmed = next.length > MAX_EVENTS ? next.slice(next.length - MAX_EVENTS) : next
      const deduped = dedupe(trimmed)
      eventsRef.current = deduped
      setEvents(deduped)
    }
  }, [])

  const {
    refreshFileTree,
    readFile,
    saveFile,
    deleteFile,
    renameFile,
    mkdir,
    createFile,
    refreshFileTreeRef,
  } = useFileActions({ activeWorkspaceRef, setFileTree })

  // ---- WebSocket connection for real-time events ----
  function connectWebSocket() {
    if (!mountedRef.current) return
    // Avoid stacking sockets: if a connection is already in flight or live,
    // bail out. A CONNECTING/OPEN socket means a prior connectWebSocket is
    // still pending its handshake — opening another would leak a dangling
    // socket. (A CLOSING socket will fire onclose shortly and schedule the
    // next attempt via the backoff path, so we don't need to handle it here.)
    const existing = wsRef.current
    if (existing && (existing.readyState === WebSocket.OPEN || existing.readyState === WebSocket.CONNECTING)) {
      return
    }
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    // Attach device credentials as query params so the backend's WebSocket
    // auth checker (sync.Hub.HandleWS) accepts the handshake from remote
    // (LAN) devices. Loopback connections bypass auth, so the host browser
    // works without these — but mobile/remote devices are rejected with 401
    // unless deviceId+secret are present. The credential is read fresh on
    // each connect so a newly-paired device doesn't need a page reload.
    const cred = getDeviceCredential()
    const wsParams = cred
      ? `?deviceId=${encodeURIComponent(cred.id)}&secret=${encodeURIComponent(cred.secret)}`
      : ''
    const wsUrl = `${protocol}//${window.location.host}/ws${wsParams}`

    const ws = new WebSocket(wsUrl)
    wsRef.current = ws

    ws.onopen = () => {
      setConnected(true)
      // Clear the reconnecting banner once the socket is back up, and record
      // the connection time so a subsequent drop can re-arm the banner.
      setReconnecting(false)
      setReconnectFailed(false)
      if (reconnectGiveupTimerRef.current) {
        clearTimeout(reconnectGiveupTimerRef.current)
        reconnectGiveupTimerRef.current = undefined
      }
      lastConnectedAtRef.current = Date.now()
      // Reset the backoff ramp — a successful connect means we're back to a
      // healthy state, so the next drop starts from the 1s base delay again.
      reconnectAttemptRef.current = 0
      // On (re)connect, re-sync sessions, pending permissions, and catch up
      // on any events missed while the socket was down. loadEvents() is now
      // safe to call here because it MERGES (cursor-based append with ID
      // dedup) instead of replacing the event list — so it never wipes
      // session-specific events already delivered via WebSocket. It fetches
      // events after the highest ID we hold, filling the gap left by a
      // disconnect (Blueprint Sec 12 — reconnection).
      loadSessions()
      loadPendingPermissions()
      loadEvents()
      // Restore the active workspace if it was never loaded — e.g. the page
      // loaded while the network was down so loadWorkspaces() failed and left
      // activeWorkspace null, then the socket came back up. Without this,
      // saveFile sends an empty workspace id and the backend returns 404
      // "Not found" (the save-after-reconnection bug).
      restoreActiveWorkspace()
    }
    ws.onclose = () => {
      if (!mountedRef.current) return
      setConnected(false)
      // Only show the reconnecting banner after the socket has connected at
      // least once — a cold-load failure (backend not up yet) should not flash
      // a "Reconnecting…" banner, but a mid-session Wi-Fi drop should.
      if (lastConnectedAtRef.current !== null) {
        setReconnecting(true)
        // Arm the give-up timer: if we don't reconnect within the window, flip
        // to the fatal banner so the user gets a manual retry affordance
        // instead of indefinite pulsing.
        if (!reconnectGiveupTimerRef.current) {
          reconnectGiveupTimerRef.current = setTimeout(() => {
            reconnectGiveupTimerRef.current = undefined
            if (mountedRef.current) setReconnectFailed(true)
          }, RECONNECT_GIVEUP_MS)
        }
      }
      // Exponential backoff with jitter (Blueprint Sec 12 — reconnection).
      // Delay grows as 1s, 2s, 4s, … capped at 30s, with ±20% jitter so a
      // fleet of clients that all dropped simultaneously don't retry in
      // lockstep. scheduleReconnect clears any prior timer so overlapping
      // disconnects don't stack multiple sockets.
      const delay = nextReconnectDelay(reconnectAttemptRef.current)
      reconnectAttemptRef.current += 1
      scheduleReconnect(delay)
    }
    ws.onerror = () => ws.close()
    ws.onmessage = (msg) => {
      try {
        const event = JSON.parse(msg.data) as AppEvent
        commitEvents((prev) => {
          // Dedupe by event id: the backend may replay events on reconnect,
          // and loadEvents() may have already fetched the same event via REST.
          // Without this, duplicate ToolStarted events with the same toolCallId
          // survive mergedEvents (which only folds ToolCompleted into ToolStarted,
          // not duplicate ToolStarted), producing duplicate toolCallId keys that
          // crash assistant-ui's useResources reconciler ("Duplicate key").
          if (event.id !== undefined && prev.some((e) => e.id === event.id)) {
            return prev
          }
          const next = prev.length >= MAX_EVENTS ? prev.slice(prev.length - MAX_EVENTS + 1) : prev.slice()
          next.push(event)
          return next
        })
        // Permission lifecycle events change the pending set — refresh it.
        if (
          event.type === 'PermissionRequested' ||
          event.type === 'PermissionGranted' ||
          event.type === 'PermissionDenied' ||
          event.type === 'PermissionTimedOut'
        ) {
          loadPendingPermissions()
        }
        // File-written events (agent created/modified a file via ACP) and
        // external file changes (FileChangedOnDisk, from the backend fs watcher)
        // trigger a debounced file-tree refresh so the explorer shows
        // new/removed files without a manual reload. Only refresh when the
        // event's workspace matches the active workspace.
        if (event.type === 'FileWritten' || event.type === 'FileChangedOnDisk') {
          const evtWs = event.workspaceId
          const active = activeWorkspaceRef.current
          if (!evtWs || !active || evtWs === active.id) {
            scheduleFileTreeRefresh()
          }
        }
        // Multi-client session-list sync. SessionCreated is broadcast to every
        // connected client (including the creator). Refresh the session list
        // only when the id is unknown — the creator already has it from REST.
        // Queue the id for ChatPanel to open as a background tab.
        if (event.type === 'SessionCreated' && event.sessionId) {
          const id = event.sessionId
          setSessions((prev) => {
            if (!prev.some((s) => s.id === id)) {
              void loadSessions()
            }
            return prev
          })
          setPendingCreatedSessionIds((prev) => (prev.includes(id) ? prev : [...prev, id]))
        }
        // SessionClosed: drop from local list and queue for tab close.
        if (event.type === 'SessionClosed' && event.sessionId) {
          const id = event.sessionId
          setSessions((prev) => prev.filter((s) => s.id !== id))
          setPendingClosedSessionIds((prev) => (prev.includes(id) ? prev : [...prev, id]))
        }
      } catch {
        // Ignore malformed messages.
      }
    }
  }

  // ---- Workspace actions ----
  const selectWorkspace = useCallback(async (ws: WorkspaceInfo) => {
    // Sync the ref BEFORE setActiveWorkspace so an immediately-following
    // call (e.g. registerWorkspace awaiting selectWorkspace then creating a
    // session) sees the new workspace without waiting for the mirror effect.
    activeWorkspaceRef.current = ws
    setActiveWorkspace(ws)
    // Persist the active workspace by id so it survives a reload.
    safeStorage.set('lai:activeWorkspace', ws.id)
    try {
      setFileTree(await api.getFileTree(ws.id))
    } catch {
      setFileTree([])
    }
  }, [])

  const registerWorkspace = useCallback(async (path: string) => {
    const ws = await api.registerWorkspace(path)
    setWorkspaces((prev) => [...prev, ws])
    await selectWorkspace(ws)
    return ws
  }, [selectWorkspace])

  /** PUT /api/workspaces/{id}/trust — updates the per-workspace preview trust
   *  state on the backend and locally patches both the workspaces list and the
   *  activeWorkspace so the UI (preview components, settings) reflect the new
   *  value without a full reload. */
  const setWorkspaceTrust = useCallback(async (workspaceId: string, trusted: boolean | null | undefined) => {
    await api.setWorkspaceTrust(workspaceId, trusted)
    const next = trusted ?? null
    setWorkspaces((prev) => prev.map((w) => w.id === workspaceId ? { ...w, trusted: next } : w))
    setActiveWorkspace((prev) => prev && prev.id === workspaceId ? { ...prev, trusted: next } : prev)
  }, [])

  // ---- Data loading methods ----
  const loadWorkspaces = useCallback(async () => {
    try {
      const ws = await api.listWorkspaces()
      setWorkspaces(ws)
      if (ws.length > 0 && !activeWorkspaceRef.current) {
        // Restore the previously active workspace from localStorage if it
        // still exists in the loaded list; otherwise fall back to the first.
        const storedId = safeStorage.get('lai:activeWorkspace')
        const match = storedId ? ws.find((w) => w.id === storedId) : undefined
        selectWorkspace(match ?? ws[0])
      }
    } catch {
      // Backend not ready yet.
    }
  }, [selectWorkspace])

  /**
   * Restores the active workspace after a WebSocket reconnect when it was
   * never loaded — e.g. the page loaded while the network was down so
   * loadWorkspaces() failed and left activeWorkspace null, then the socket
   * came back up. Without this, saveFile sends an empty workspace id and the
   * backend returns 404 "Not found" (the save-after-reconnection bug).
   *
   * Uses activeWorkspaceRef instead of the activeWorkspace state so the check
   * always sees the current value regardless of which render's closure the
   * onopen handler captured (the onopen closure is created once per
   * connectWebSocket call and can be stale by the time it fires). If the
   * persisted id no longer exists in the fresh list (e.g. the daemon
   * restarted with different workspace registrations), falls back to the
   * first available workspace so saving still works.
   */
  const restoreActiveWorkspace = useCallback(async () => {
    if (activeWorkspaceRef.current) return
    try {
      const ws = await api.listWorkspaces()
      setWorkspaces(ws)
      if (ws.length === 0) return
      const storedId = safeStorage.get('lai:activeWorkspace')
      const match = storedId ? ws.find((w) => w.id === storedId) : undefined
      selectWorkspace(match ?? ws[0])
    } catch {
      // Backend still not ready; the next reconnect will retry.
    }
  }, [selectWorkspace])

  const loadAgents = useCallback(async () => {
    try {
      setAgents(await api.listAgents())
    } catch {
      // No agents registered yet — that's OK.
    }
  }, [])

  const addAgent = useCallback(async (agent: Agent) => {
    await api.addAgent(agent)
    await loadAgents()
  }, [loadAgents])

  const deleteAgent = useCallback(async (agentId: string) => {
    await api.deleteAgent(agentId)
    await loadAgents()
  }, [loadAgents])

  const autodetectAgents = useCallback(async () => {
    const detected = await api.autodetectAgents()
    return detected
  }, [])

  const loadDevices = useCallback(async () => {
    try {
      setDevices(await api.listDevices())
    } catch {
      // No devices paired yet.
    }
  }, [])

  const loadEvents = useCallback(async () => {
    try {
      const afterId =
        eventsRef.current.length > 0
          ? Math.max(...eventsRef.current.map((e) => e.id ?? 0))
          : 0
      // First load begins at the durable tail. Reconnects then page forward,
      // ensuring a long disconnect cannot drop a burst larger than one page.
      let page = afterId === 0
        ? await api.getEvents(-1, EVENT_PAGE_SIZE)
        : await api.getEvents(afterId, EVENT_PAGE_SIZE)
      while (page.length > 0) {
        const existingIds = new Set(eventsRef.current.map((e) => e.id))
        const fresh = page.filter((e) => !existingIds.has(e.id))
        if (fresh.length > 0) commitEvents([...eventsRef.current, ...fresh].sort(byEventId))
        if (afterId === 0 || page.length < EVENT_PAGE_SIZE) break
        page = await api.getEvents(Math.max(...page.map((e) => e.id ?? 0)), EVENT_PAGE_SIZE)
      }
    } catch {
      // Event store may be empty.
    }
  }, [commitEvents])

  const loadSessions = useCallback(async () => {
    try {
      setSessions(await api.listSessions())
    } catch {
      // No sessions yet.
    }
  }, [])

  const loadPendingPermissions = useCallback(async () => {
    try {
      setPendingPermissions(await api.getPendingPermissions())
    } catch {
      // None pending.
    }
  }, [])

  /** Loads the session tail, preserving WebSocket events that raced the fetch. */
  const loadSessionEvents = useCallback(async (sessionId: string) => {
    setHasOlderSessionEvents(false)
    try {
      const sessionEvts = await api.getSessionEvents(sessionId, -1, EVENT_PAGE_SIZE)
      const maxFetchedId = sessionEvts.length > 0
        ? Math.max(...sessionEvts.map((e) => e.id ?? 0))
        : 0
      commitEvents([
        ...sessionEvts,
        ...eventsRef.current.filter(
          (e) => e.sessionId !== sessionId || (e.id ?? 0) > maxFetchedId,
        ),
      ].sort(byEventId))
      setHasOlderSessionEvents(sessionEvts.length === EVENT_PAGE_SIZE)
    } catch {
      // Session may not have events yet.
    }
  }, [commitEvents])

  /** Prepends one older page; reverse queries are returned in chronological order. */
  const loadOlderSessionEvents = useCallback(async (sessionId: string) => {
    const sessionEvents = eventsRef.current.filter((e) => e.sessionId === sessionId)
    const oldestId = Math.min(...sessionEvents.map((e) => e.id ?? 0))
    if (!Number.isFinite(oldestId)) return
    try {
      const older = await api.getSessionEvents(sessionId, -oldestId - 1, EVENT_PAGE_SIZE)
      const existingIds = new Set(eventsRef.current.map((e) => e.id))
      const fresh = older.filter((e) => !existingIds.has(e.id))
      if (fresh.length > 0) commitEvents([...eventsRef.current, ...fresh].sort(byEventId))
      setHasOlderSessionEvents(older.length === EVENT_PAGE_SIZE)
    } catch {
      // The loaded history remains usable if an older-page request fails.
    }
  }, [commitEvents])

  const {
    createSession,
    sendPrompt,
    uploadFile,
    cancelSession,
    renameSession,
    rebindSession,
    switchModel,
    reportContext,
    deleteSession,
    exportSession,
    reportContextTimerRef,
  } = useSessionActions({
    activeWorkspaceRef,
    setSessions,
    loadSessions,
    commitEvents,
    eventsRef,
  })

  // ---- Pairing actions ----
  const verifyPasscode = useCallback(
    async (passcode: string, deviceName: string) => {
      const cred = await api.verifyPasscode(passcode, deviceName)
      // Store credential in sessionStorage (Blueprint Sec 19 — browser-stored).
      // Uses the lai: prefix for consistency with other persisted keys.
      sessionStorage.setItem('lai:deviceCredential', JSON.stringify(cred))
      await loadDevices()
      return cred
    },
    [loadDevices],
  )

  const revokeDevice = useCallback(async (deviceId: string) => {
    await api.revokeDevice(deviceId)
    setDevices((prev) => prev.filter((d) => d.id !== deviceId))
  }, [])

  // ---- Permission actions ----
  const respondPermission = useCallback(async (requestId: string, decision: string) => {
    await api.respondPermission(requestId, decision)
  }, [])

  // ---- Load initial data on mount ----
  // This effect must run EXACTLY ONCE on mount: it kicks off the initial data
  // loads and opens the single WebSocket connection (connectWebSocket recreates
  // the socket, so re-running it would leak/duplicate connections). The loader
  // functions and connectWebSocket are stable for our purposes — they read
  // current values through refs (activeWorkspaceRef, eventsRef, etc.) rather
  // than captured render state — so listing them as deps would only risk
  // reconnect-on-every-render without changing behavior. Hence the two
  // targeted disables below (exhaustive-deps + set-state-in-effect) instead
  // of a file-level blanket disable.
  useEffect(() => {
    mountedRef.current = true
    // eslint-disable-next-line react-hooks/set-state-in-effect -- run-once-on-mount: stable loaders call setState but only fire on mount, not during render.
    loadWorkspaces()
    loadAgents()
    loadDevices()
    loadEvents()
    loadSessions()
    loadPendingPermissions()
    connectWebSocket()
    // Reconnect immediately when the network comes back online or the tab
    // returns to the foreground — these are strong signals that a dropped
    // socket can now succeed, so we shouldn't wait out the backoff timer.
    window.addEventListener('online', onOnline)
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      // Mark unmounted first so ws.onclose (fired by close()) short-circuits
      // instead of scheduling a reconnect against a torn-down hook.
      mountedRef.current = false
      window.removeEventListener('online', onOnline)
      document.removeEventListener('visibilitychange', onVisibility)
      if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current)
      if (treeRefreshTimerRef.current) clearTimeout(treeRefreshTimerRef.current)
      // eslint-disable-next-line react-hooks/exhaustive-deps -- reportContextTimerRef is a stable debounce timer ref from useSessionActions, not a React node; reading .current at cleanup is intentional.
      if (reportContextTimerRef.current) clearTimeout(reportContextTimerRef.current)
      wsRef.current?.close()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- run-once-on-mount: opens the single WebSocket; must not re-run on every render.
  }, [])

  const consumeSessionCreated = useCallback((id: string) => {
    setPendingCreatedSessionIds((prev) => prev.filter((x) => x !== id))
  }, [])
  const consumeSessionClosed = useCallback((id: string) => {
    setPendingClosedSessionIds((prev) => prev.filter((x) => x !== id))
  }, [])

  return {
    // State
    workspaces,
    activeWorkspace,
    fileTree,
    agents,
    sessions,
    events,
    hasOlderSessionEvents,
    devices,
    pendingPermissions,
    connected,
    reconnecting,
    reconnectFailed,
    reconnectNow,
    // Multi-client session sync queues + drain helpers.
    pendingCreatedSessionIds,
    pendingClosedSessionIds,
    consumeSessionCreated,
    consumeSessionClosed,

    // Actions
    selectWorkspace,
    registerWorkspace,
    setWorkspaceTrust,
    readFile,
    saveFile,
    deleteFile,
    renameFile,
    mkdir,
    createFile,
    refreshFileTree,
    createSession,
    sendPrompt,
    uploadFile,
    cancelSession,
    renameSession,
    rebindSession,
    switchModel,
    deleteSession,
    exportSession,
    reportContext,
    verifyPasscode,
    revokeDevice,
    respondPermission,
    loadWorkspaces,
    loadAgents,
    addAgent,
    deleteAgent,
    autodetectAgents,
    loadDevices,
    loadSessions,
    loadSessionEvents,
    loadOlderSessionEvents,
    loadPendingPermissions,
  }
}
