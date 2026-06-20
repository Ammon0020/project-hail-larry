import { useCallback, useEffect, useRef, useState } from 'react'
import { api, type AppEvent, type WorkspaceInfo, type FileNode, type AgentInfo, type SessionInfo, type DeviceCredential } from '@/lib/api'

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
  const [connected, setConnected] = useState(false)

  const wsRef = useRef<WebSocket | null>(null)
  const eventsRef = useRef<AppEvent[]>([])

  // ---- Load initial data on mount ----
  useEffect(() => {
    loadWorkspaces()
    loadAgents()
    loadDevices()
    loadEvents()
    connectWebSocket()
    return () => {
      wsRef.current?.close()
    }
  }, [])

  // ---- WebSocket connection for real-time events ----
  const connectWebSocket = useCallback(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const wsUrl = `${protocol}//${window.location.host}/ws`

    const ws = new WebSocket(wsUrl)
    wsRef.current = ws

    ws.onopen = () => setConnected(true)
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
      } catch {
        // Ignore malformed messages.
      }
    }
  }, [])

  // ---- Data loading methods ----
  const loadWorkspaces = useCallback(async () => {
    try {
      const ws = await api.listWorkspaces()
      setWorkspaces(ws)
      if (ws.length > 0 && !activeWorkspace) {
        selectWorkspace(ws[0])
      }
    } catch {
      // Backend not ready yet.
    }
  }, [activeWorkspace])

  const loadAgents = useCallback(async () => {
    try {
      setAgents(await api.listAgents())
    } catch {
      // No agents registered yet — that's OK.
    }
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
      const evts = await api.getEvents(0, 200)
      eventsRef.current = evts
      setEvents(evts)
    } catch {
      // Event store may be empty.
    }
  }, [])

  // ---- Workspace actions ----
  const selectWorkspace = useCallback(async (ws: WorkspaceInfo) => {
    setActiveWorkspace(ws)
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

  // ---- Session actions ----
  const createSession = useCallback(async (agentId: string, modelId: string) => {
    const wsId = activeWorkspace?.id || ''
    const session = await api.createSession(agentId, modelId, wsId)
    setSessions((prev) => [...prev, session])
    return session
  }, [activeWorkspace])

  const sendPrompt = useCallback(async (sessionId: string, content: string) => {
    // Optimistically add the user message to the event list.
    const optimisticEvent: AppEvent = {
      type: 'PromptSubmitted',
      sessionId,
      role: 'user',
      content,
    }
    eventsRef.current = [...eventsRef.current, optimisticEvent]
    setEvents(eventsRef.current)

    try {
      await api.sendPrompt(sessionId, content)
    } catch {
      // The event was still persisted server-side; the error is non-fatal.
    }
  }, [])

  const cancelSession = useCallback(async (sessionId: string) => {
    await api.cancelSession(sessionId)
  }, [])

  // ---- Pairing actions ----
  const verifyPasscode = useCallback(async (passcode: string, deviceName: string) => {
    const cred = await api.verifyPasscode(passcode, deviceName)
    // Store credential in localStorage (Blueprint Sec 19 — browser-stored).
    localStorage.setItem('deviceCredential', JSON.stringify(cred))
    await loadDevices()
    return cred
  }, [loadDevices])

  const revokeDevice = useCallback(async (deviceId: string) => {
    await api.revokeDevice(deviceId)
    setDevices((prev) => prev.filter((d) => d.id !== deviceId))
  }, [])

  // ---- Permission actions ----
  const respondPermission = useCallback(async (requestId: string, decision: string) => {
    await api.respondPermission(requestId, decision)
  }, [])

  return {
    // State
    workspaces,
    activeWorkspace,
    fileTree,
    agents,
    sessions,
    events,
    devices,
    connected,

    // Actions
    selectWorkspace,
    registerWorkspace,
    createSession,
    sendPrompt,
    cancelSession,
    verifyPasscode,
    revokeDevice,
    respondPermission,
    loadWorkspaces,
    loadAgents,
    loadDevices,
  }
}
