/* eslint-disable react-hooks/exhaustive-deps */
import { useEffect, useRef, useState } from 'react'
import { api, type AppEvent, type WorkspaceInfo, type FileNode, type AgentInfo, type SessionInfo, type DeviceCredential, type PendingPermission } from '@/lib/api'

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
      // Re-sync after (re)connect so no events or pending prompts are missed
      // while the socket was down (Blueprint Sec 12 — reconnection).
      loadEvents()
      loadSessions()
      loadPendingPermissions()
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
      } catch {
        // Ignore malformed messages.
      }
    }
  }

  // ---- Workspace actions ----
  async function selectWorkspace(ws: WorkspaceInfo) {
    setActiveWorkspace(ws)
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
        selectWorkspace(ws[0])
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
      const evts = await api.getEvents(0, 200)
      eventsRef.current = evts
      setEvents(evts)
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
   */
  async function loadSessionEvents(sessionId: string) {
    try {
      const sessionEvts = await api.getSessionEvents(sessionId, 0, 200)
      // Merge: keep events from other sessions, replace events for this session.
      eventsRef.current = [
        ...eventsRef.current.filter((e) => e.sessionId !== sessionId),
        ...sessionEvts,
      ]
      setEvents(eventsRef.current)
    } catch {
      // Session may not have events yet.
    }
  }

  // ---- File actions ----
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
    await api.sendPrompt(sessionId, content)
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
