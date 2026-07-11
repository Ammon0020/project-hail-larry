import { useEffect, useRef, useState } from 'react'
import { api, type AppEvent, type WorkspaceInfo, type FileNode, type AgentInfo, type SessionInfo, type DeviceCredential, type PendingPermission, type UploadResult, type EditorSelection } from '@/lib/api'
import type { Attachment } from '@/types'

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

/**
 * Returns true when an error message indicates the targeted session no longer
 * exists in the backend (404 "session not found: sess-…"). Used to recover
 * from a stale activeSessionId persisted in localStorage.
 */
function isSessionNotFound(message: string): boolean {
  const lower = message.toLowerCase()
  return lower.includes('session not found') || lower.includes('not found')
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
  const [agents, setAgents] = useState<AgentInfo[]>([])
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  const [events, setEvents] = useState<AppEvent[]>([])
  const [devices, setDevices] = useState<DeviceCredential[]>([])
  const [pendingPermissions, setPendingPermissions] = useState<PendingPermission[]>([])
  const [connected, setConnected] = useState(false)
  // reconnecting is true only while the WebSocket is closed AND retrying after a
  // prior successful connection. It stays false on the initial connect attempt
  // (before the first onopen) so the UI does not flash a "Reconnecting…" banner
  // on a cold load. lastConnectedAt records the timestamp of the last successful
  // ws.onopen and gates the reconnecting flag.
  const [reconnecting, setReconnecting] = useState(false)
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
  function commitEvents(next: AppEvent[]) {
    const trimmed = next.length > MAX_EVENTS ? next.slice(next.length - MAX_EVENTS) : next
    eventsRef.current = trimmed
    setEvents(trimmed)
  }

  // ---- Load initial data on mount ----
  // This effect must run EXACTLY ONCE on mount: it kicks off the initial data
  // loads and opens the single WebSocket connection (connectWebSocket recreates
  // the socket, so re-running it would leak/duplicate connections). The loader
  // functions and connectWebSocket are stable for our purposes — they read
  // current values through refs (activeWorkspaceRef, eventsRef, etc.) rather
  // than captured render state — so listing them as deps would only risk
  // reconnect-on-every-render without changing behavior. Hence the single
  // targeted disable below instead of a file-level blanket disable.
  useEffect(() => {
    mountedRef.current = true
    loadWorkspaces()
    loadAgents()
    loadDevices()
    loadEvents()
    loadSessions()
    loadPendingPermissions()
    connectWebSocket()
    return () => {
      // Mark unmounted first so ws.onclose (fired by close()) short-circuits
      // instead of scheduling a reconnect against a torn-down hook.
      mountedRef.current = false
      if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current)
      wsRef.current?.close()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- run-once-on-mount: opens the single WebSocket; must not re-run on every render.
  }, [])

  // ---- WebSocket connection for real-time events ----
  function connectWebSocket() {
    if (!mountedRef.current) return
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const wsUrl = `${protocol}//${window.location.host}/ws`

    const ws = new WebSocket(wsUrl)
    wsRef.current = ws

    ws.onopen = () => {
      setConnected(true)
      // Clear the reconnecting banner once the socket is back up, and record
      // the connection time so a subsequent drop can re-arm the banner.
      setReconnecting(false)
      lastConnectedAtRef.current = Date.now()
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
      }
      // Reconnect after 3 seconds (Blueprint Sec 12 — reconnection). Clear any
      // prior timer so overlapping disconnects don't stack multiple sockets.
      if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current)
      reconnectTimerRef.current = setTimeout(() => connectWebSocket(), 3000)
    }
    ws.onerror = () => ws.close()
    ws.onmessage = (msg) => {
      try {
        const event = JSON.parse(msg.data) as AppEvent
        commitEvents([...eventsRef.current, event])
        // Permission lifecycle events change the pending set — refresh it.
        if (
          event.type === 'PermissionRequested' ||
          event.type === 'PermissionGranted' ||
          event.type === 'PermissionDenied'
        ) {
          loadPendingPermissions()
        }
        // File-written events (agent created/modified a file via ACP) and
        // external file changes (FileChangedOnDisk, from the backend fs watcher)
        // trigger a file-tree refresh so the explorer shows new/removed files
        // without a manual reload. Only refresh when the event's workspace
        // matches the active workspace.
        if (event.type === 'FileWritten' || event.type === 'FileChangedOnDisk') {
          const evtWs = event.workspaceId
          const active = activeWorkspaceRef.current
          if (!evtWs || !active || evtWs === active.id) {
            refreshFileTree()
          }
        }
      } catch {
        // Ignore malformed messages.
      }
    }
  }

  // ---- Workspace actions ----
  async function selectWorkspace(ws: WorkspaceInfo) {
    setActiveWorkspace(ws)
    // Persist the active workspace by id so it survives a reload.
    localStorage.setItem('lai:activeWorkspace', ws.id)
    try {
      setFileTree(await api.getFileTree(ws.id))
    } catch {
      setFileTree([])
    }
  }

  async function registerWorkspace(path: string) {
    const ws = await api.registerWorkspace(path)
    setWorkspaces((prev) => [...prev, ws])
    await selectWorkspace(ws)
    return ws
  }

  // ---- Data loading methods ----
  async function loadWorkspaces() {
    try {
      const ws = await api.listWorkspaces()
      setWorkspaces(ws)
      if (ws.length > 0 && !activeWorkspace) {
        // Restore the previously active workspace from localStorage if it
        // still exists in the loaded list; otherwise fall back to the first.
        const storedId = localStorage.getItem('lai:activeWorkspace')
        const match = storedId ? ws.find((w) => w.id === storedId) : undefined
        selectWorkspace(match ?? ws[0])
      }
    } catch {
      // Backend not ready yet.
    }
  }

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
  async function restoreActiveWorkspace() {
    if (activeWorkspaceRef.current) return
    try {
      const ws = await api.listWorkspaces()
      setWorkspaces(ws)
      if (ws.length === 0) return
      const storedId = localStorage.getItem('lai:activeWorkspace')
      const match = storedId ? ws.find((w) => w.id === storedId) : undefined
      selectWorkspace(match ?? ws[0])
    } catch {
      // Backend still not ready; the next reconnect will retry.
    }
  }

  async function loadAgents() {
    try {
      setAgents(await api.listAgents())
    } catch {
      // No agents registered yet — that's OK.
    }
  }

  async function addAgent(agent: AgentInfo) {
    await api.addAgent(agent)
    await loadAgents()
  }

  async function deleteAgent(agentId: string) {
    await api.deleteAgent(agentId)
    await loadAgents()
  }

  async function autodetectAgents() {
    const detected = await api.autodetectAgents()
    return detected
  }

  async function loadDevices() {
    try {
      setDevices(await api.listDevices())
    } catch {
      // No devices paired yet.
    }
  }

  async function loadEvents() {
    try {
      // Cursor-based fetch: retrieve events AFTER the highest ID we already
      // hold. On the initial mount eventsRef is empty (afterId=0 → first
      // 1000 events), matching the previous behavior. On WebSocket reconnect
      // this catches any events missed while the socket was down — without
      // wiping session-specific events already delivered in real time.
      //
      // Merging (appending) instead of replacing is what makes this safe to
      // call on reconnect: a global fetch that replaced the list would discard
      // StreamUpdate events delivered via WebSocket for non-active sessions
      // while the user was viewing another conversation (the "frozen stream"
      // bug). Appending preserves them.
      const afterId =
        eventsRef.current.length > 0
          ? Math.max(...eventsRef.current.map((e) => e.id ?? 0))
          : 0
      const evts = await api.getEvents(afterId, 1000)
      if (evts.length === 0) return
      // Dedupe by ID: a WebSocket event may have arrived between computing
      // the cursor and the fetch returning, and the fetch would also include
      // it (recordEvent persists before broadcasting). Keep only fetched
      // events whose IDs we don't already have to avoid duplicates.
      const existingIds = new Set(eventsRef.current.map((e) => e.id))
      const fresh = evts.filter((e) => !existingIds.has(e.id))
      if (fresh.length > 0) {
        commitEvents([...eventsRef.current, ...fresh])
      }
    } catch {
      // Event store may be empty.
    }
  }

  async function loadSessions() {
    try {
      setSessions(await api.listSessions())
    } catch {
      // No sessions yet.
    }
  }

  async function loadPendingPermissions() {
    try {
      setPendingPermissions(await api.getPendingPermissions())
    } catch {
      // None pending.
    }
  }

  /**
   * Loads events for a specific session from the backend and merges them
   * into the local event list so the chat panel can render them.
   *
   * Merges by event ID instead of blindly replacing all events for the
   * session. This prevents a race condition where WebSocket-delivered events
   * (e.g. PromptSubmitted) arrive between the time the fetch is initiated and
   * the time it completes: the fetched list would be stale and overwrite the
   * newer WebSocket events. We keep events for this session whose IDs are
   * higher than the highest fetched ID (they arrived after the fetch started),
   * plus the freshly fetched events.
   */
  async function loadSessionEvents(sessionId: string) {
    try {
      const sessionEvts = await api.getSessionEvents(sessionId, 0, 1000)
      // Merge: keep events from other sessions, plus events for this session
      // that arrived via WebSocket after the fetch was initiated (they have
      // IDs higher than the fetched events).
      const maxFetchedId = sessionEvts.length > 0
        ? Math.max(...sessionEvts.map((e) => e.id ?? 0))
        : 0
      commitEvents([
        ...sessionEvts,
        ...eventsRef.current.filter(
          (e) => e.sessionId !== sessionId || (e.id ?? 0) > maxFetchedId,
        ),
      ])
    } catch {
      // Session may not have events yet.
    }
  }

  // ---- File actions ----
  /**
   * Reloads the file tree for the active workspace from the backend. Called
   * after a FileWritten event so the explorer reflects agent-created files
   * without a manual refresh.
   */
  async function refreshFileTree() {
    const ws = activeWorkspaceRef.current
    if (!ws) return
    try {
      setFileTree(await api.getFileTree(ws.id))
    } catch {
      // Workspace may have been removed; leave the tree as-is.
    }
  }

  async function readFile(path: string, workspaceId?: string) {
    const wsId = workspaceId || activeWorkspace?.id || ''
    return await api.readFile(wsId, path)
  }

  async function saveFile(path: string, content: string, expectedRevision: number, workspaceId?: string) {
    const wsId = workspaceId || activeWorkspace?.id || ''
    return await api.saveFile(wsId, path, content, expectedRevision)
  }

  // ---- Session actions ----
  async function createSession(agentId: string, modelId: string) {
    const wsId = activeWorkspace?.id || ''
    const session = await api.createSession(agentId, modelId, wsId)
    setSessions((prev) => [...prev, session])
    return session
  }

  async function sendPrompt(sessionId: string, content: string, attachments?: Attachment[]) {
    try {
      await api.sendPrompt(sessionId, content, attachments)
    } catch (err) {
      // A stale activeSessionId (e.g. after a daemon restart that wiped
      // conversations.json, or a deleted session) makes the backend return
      // 404 "session not found: sess-…". Recover gracefully: clear the
      // persisted id so the UI resets to the new-chat state, and surface a
      // friendly message instead of the raw error string.
      const msg = err instanceof Error ? err.message : String(err)
      if (isSessionNotFound(msg)) {
        localStorage.removeItem('lai:activeSessionId')
        throw new Error('This conversation is no longer available. Start a new chat.', {
          cause: err,
        })
      }
      throw err
    }
  }

  /** Uploads a file to a session's upload store. Thin wrapper around
   *  api.uploadFile so components can call it through the hook and share the
   *  hook's session-recovery semantics. */
  async function uploadFile(sessionId: string, file: File): Promise<UploadResult> {
    return await api.uploadFile(sessionId, file)
  }

  async function cancelSession(sessionId: string) {
    await api.cancelSession(sessionId)
  }

  async function renameSession(sessionId: string, name: string) {
    await api.patchSession(sessionId, { name })
    await loadSessions()
  }

  async function rebindSession(
    sessionId: string,
    agentId: string,
    modelId: string,
    maxTransferBytes?: number,
  ) {
    await api.patchSession(sessionId, { agentId, modelId, maxTransferBytes })
    await loadSessions()
  }

  /**
   * Switches the model on a live session without restarting the agent process.
   * Unlike rebindSession, this preserves the full conversation context — the
   * agent keeps its in-memory state and just uses the new model for subsequent
   * turns. Sends a model-only PATCH (no agentId) so the backend routes to
   * SwitchModel instead of RebindSession.
   */
  async function switchModel(sessionId: string, modelId: string) {
    await api.patchSession(sessionId, { modelId })
    await loadSessions()
  }

  // Debounce timer for reportContext so rapid tab switches / edits don't
  // flood the backend with context updates.
  const reportContextTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  /**
   * Reports the current open files and recent edits to the backend so the
   * context middleware can inject them into the next agent prompt. Debounced
   * by ~1s to coalesce rapid tab switches and keystroke-driven unsaved-state
   * changes into a single request. The optional selection is the user's
   * current editor selection (sent as a resource block by the backend).
   */
  function reportContext(
    sessionId: string,
    openFiles: string[],
    recentEdits: string[],
    selection?: EditorSelection,
  ) {
    if (reportContextTimerRef.current) clearTimeout(reportContextTimerRef.current)
    reportContextTimerRef.current = setTimeout(async () => {
      try {
        await api.reportSessionContext(sessionId, openFiles, recentEdits, selection)
      } catch {
        // Non-fatal — context reporting is best-effort.
      }
    }, 1000)
  }

  async function deleteSession(sessionId: string) {
    await api.closeSession(sessionId)
    setSessions((prev) => prev.filter((s) => s.id !== sessionId))
    // Drop the deleted conversation's events from the local cache. Filtering
    // only shrinks the list, but route it through commitEvents anyway so every
    // event-log mutation goes through the single capped path.
    commitEvents(eventsRef.current.filter((e) => e.sessionId !== sessionId))
  }

  /** Exports a conversation as a markdown transcript. The backend renders the
   *  full event history into a readable transcript and the api client triggers
   *  a browser download of the resulting text/markdown blob. */
  async function exportSession(sessionId: string) {
    await api.exportSession(sessionId)
  }

  // ---- Pairing actions ----
  async function verifyPasscode(passcode: string, deviceName: string) {
    const cred = await api.verifyPasscode(passcode, deviceName)
    // Store credential in localStorage (Blueprint Sec 19 — browser-stored).
    // Uses the lai: prefix for consistency with other persisted keys.
    localStorage.setItem('lai:deviceCredential', JSON.stringify(cred))
    await loadDevices()
    return cred
  }

  async function revokeDevice(deviceId: string) {
    await api.revokeDevice(deviceId)
    setDevices((prev) => prev.filter((d) => d.id !== deviceId))
  }

  // ---- Permission actions ----
  async function respondPermission(requestId: string, decision: string) {
    await api.respondPermission(requestId, decision)
  }

  return {
    // State
    workspaces,
    activeWorkspace,
    fileTree,
    agents,
    sessions,
    events,
    devices,
    pendingPermissions,
    connected,
    reconnecting,

    // Actions
    selectWorkspace,
    registerWorkspace,
    readFile,
    saveFile,
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
    loadPendingPermissions,
  }
}
