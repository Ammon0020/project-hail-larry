/* eslint-disable react-hooks/exhaustive-deps */
import { useEffect, useRef, useState } from 'react'
import { api, type AppEvent, type WorkspaceInfo, type FileNode, type AgentInfo, type SessionInfo, type DeviceCredential, type PendingPermission } from '@/lib/api'

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

  const wsRef = useRef<WebSocket | null>(null)
  const eventsRef = useRef<AppEvent[]>([])
  // Mirror activeWorkspace in a ref so the WebSocket onmessage closure (created
  // once at mount) can read the current value instead of a stale snapshot.
  const activeWorkspaceRef = useRef<WorkspaceInfo | null>(null)
  useEffect(() => {
    activeWorkspaceRef.current = activeWorkspace
  }, [activeWorkspace])

  // ---- Load initial data on mount ----
  useEffect(() => {
    loadWorkspaces()
    loadAgents()
    loadDevices()
    loadEvents()
    loadSessions()
    loadPendingPermissions()
    connectWebSocket()
    return () => {
      wsRef.current?.close()
    }
  }, [])

  // ---- WebSocket connection for real-time events ----
  function connectWebSocket() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const wsUrl = `${protocol}//${window.location.host}/ws`

    const ws = new WebSocket(wsUrl)
    wsRef.current = ws

    ws.onopen = () => {
      setConnected(true)
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
    }
    ws.onclose = () => {
      setConnected(false)
      // Reconnect after 3 seconds (Blueprint Sec 12 — reconnection).
      setTimeout(() => connectWebSocket(), 3000)
    }
    ws.onerror = () => ws.close()
    ws.onmessage = (msg) => {
      try {
        const event = JSON.parse(msg.data) as AppEvent
        eventsRef.current = [...eventsRef.current, event]
        setEvents(eventsRef.current)
        // Permission lifecycle events change the pending set — refresh it.
        if (
          event.type === 'PermissionRequested' ||
          event.type === 'PermissionGranted' ||
          event.type === 'PermissionDenied'
        ) {
          loadPendingPermissions()
        }
        // File-written events (agent created/modified a file via ACP) trigger a
        // file-tree refresh so the explorer shows the new file without a manual
        // reload. Only refresh when the event's workspace matches the active
        // workspace (the agent only writes within its session's workspace).
        if (event.type === 'FileWritten') {
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
          ? Math.max(...eventsRef.current.map((e) => e.id))
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
        eventsRef.current = [...eventsRef.current, ...fresh]
        setEvents(eventsRef.current)
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
        ? Math.max(...sessionEvts.map((e) => e.id))
        : 0
      eventsRef.current = [
        ...sessionEvts,
        ...eventsRef.current.filter(
          (e) => e.sessionId !== sessionId || e.id > maxFetchedId,
        ),
      ]
      setEvents(eventsRef.current)
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

  async function readFile(path: string) {
    const wsId = activeWorkspace?.id || ''
    return await api.readFile(wsId, path)
  }

  async function saveFile(path: string, content: string, expectedRevision: number) {
    const wsId = activeWorkspace?.id || ''
    return await api.saveFile(wsId, path, content, expectedRevision)
  }

  // ---- Session actions ----
  async function createSession(agentId: string, modelId: string) {
    const wsId = activeWorkspace?.id || ''
    const session = await api.createSession(agentId, modelId, wsId)
    setSessions((prev) => [...prev, session])
    return session
  }

  async function sendPrompt(sessionId: string, content: string) {
    try {
      await api.sendPrompt(sessionId, content)
    } catch (err) {
      // A stale activeSessionId (e.g. after a daemon restart that wiped
      // conversations.json, or a deleted session) makes the backend return
      // 404 "session not found: sess-…". Recover gracefully: clear the
      // persisted id so the UI resets to the new-chat state, and surface a
      // friendly message instead of the raw error string.
      const msg = err instanceof Error ? err.message : String(err)
      if (isSessionNotFound(msg)) {
        localStorage.removeItem('activeSessionId')
        throw new Error('This conversation is no longer available. Start a new chat.', {
          cause: err,
        })
      }
      throw err
    }
  }

  async function cancelSession(sessionId: string) {
    await api.cancelSession(sessionId)
  }

  async function renameSession(sessionId: string, name: string) {
    await api.patchSession(sessionId, { name })
    await loadSessions()
  }

  async function rebindSession(sessionId: string, agentId: string, modelId: string) {
    await api.patchSession(sessionId, { agentId, modelId })
    await loadSessions()
  }

  async function deleteSession(sessionId: string) {
    await api.closeSession(sessionId)
    setSessions((prev) => prev.filter((s) => s.id !== sessionId))
    // Drop the deleted conversation's events from the local cache.
    eventsRef.current = eventsRef.current.filter((e) => e.sessionId !== sessionId)
    setEvents(eventsRef.current)
  }

  // ---- Pairing actions ----
  async function verifyPasscode(passcode: string, deviceName: string) {
    const cred = await api.verifyPasscode(passcode, deviceName)
    // Store credential in localStorage (Blueprint Sec 19 — browser-stored).
    localStorage.setItem('deviceCredential', JSON.stringify(cred))
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

    // Actions
    selectWorkspace,
    registerWorkspace,
    readFile,
    saveFile,
    createSession,
    sendPrompt,
    cancelSession,
    renameSession,
    rebindSession,
    deleteSession,
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
