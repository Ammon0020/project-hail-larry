/**
 * API client for the Local Agent Interface Go daemon.
 * All endpoints are relative to the same origin (served by the Go server).
 */

const API_BASE = '/api'

/** Generic fetch wrapper with JSON parsing. */
async function apiFetch<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
  })
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new Error(body.error || `HTTP ${res.status}`)
  }
  const data = await res.json()
  // Go's json.Encoder serializes nil slices as null — coerce to [] for array types.
  if (data === null) {
    return [] as unknown as T
  }
  return data as T
}

// ---- Types matching the Go structs ----

export interface WorkspaceInfo {
  id: string
  path: string
  name: string
}

export interface FileNode {
  name: string
  type: 'folder' | 'file'
  path: string
  children?: FileNode[]
}

export interface AgentInfo {
  id: string
  name: string
  command: string
  args: string[]
  models: AgentModel[]
  warning?: string
}

export interface AgentModel {
  id: string
  name: string
}

export interface SessionInfo {
  id: string
  name: string
  status: string
  agentId?: string
  modelId?: string
  updatedAt?: string
}

export interface AppEvent {
  id: number
  type: string
  sessionId: string
  role?: string
  content?: string
  streaming?: boolean
  tool?: string
  target?: string
  summary?: string
  command?: string
  options?: string[]
  requestId?: string
  toolKind?: string
  toolCallId?: string
  thought?: boolean
  exitCode?: number
  workspaceId?: string
}

export interface PermissionOptionInfo {
  id: string
  name: string
  kind: string
}

export interface PendingPermission {
  id: string
  sessionId: string
  tool: string
  command?: string
  target?: string
  options: string[]
  optionDetails?: PermissionOptionInfo[]
}

export interface DeviceCredential {
  id: string
  name: string
  secret: string
  pairedAt: string
}

export interface PairingSession {
  id: string
  token: string
  passcode: string
  url: string
  qrPath: string
  createdAt: string
  expiresAt: string
  used: boolean
}

// ---- API methods ----

export const api = {
  // Health
  health: () => apiFetch<{ status: string }>('/health'.replace('/api', '')),

  // Workspaces
  listWorkspaces: () => apiFetch<WorkspaceInfo[]>('/workspaces'),
  registerWorkspace: (path: string) =>
    apiFetch<WorkspaceInfo>('/workspaces', {
      method: 'POST',
      body: JSON.stringify({ path }),
    }),
  getFileTree: (workspaceId: string) =>
    apiFetch<FileNode[]>(`/workspaces/${workspaceId}/files`),
  readFile: (workspaceId: string, path: string) =>
    apiFetch<{ content: string; revision: number; path: string }>(
      `/workspaces/${workspaceId}/file?path=${encodeURIComponent(path)}`,
    ),
  saveFile: (workspaceId: string, path: string, content: string, expectedRevision: number) =>
    apiFetch<{ revision: number; path: string }>(`/workspaces/${workspaceId}/file`, {
      method: 'POST',
      body: JSON.stringify({ path, content, expectedRevision }),
    }),

  // Events
  getEvents: (afterId = 0, limit = 100) =>
    apiFetch<AppEvent[]>(`/events?after=${afterId}&limit=${limit}`),
  getSessionEvents: (sessionId: string, afterId = 0, limit = 100) =>
    apiFetch<AppEvent[]>(`/events/${sessionId}?after=${afterId}&limit=${limit}`),

  // Agents & Sessions
  listAgents: () => apiFetch<AgentInfo[]>('/agents'),
  addAgent: (agent: AgentInfo) =>
    apiFetch<AgentInfo>('/agents', {
      method: 'POST',
      body: JSON.stringify(agent),
    }),
  deleteAgent: (agentId: string) =>
    apiFetch<{ status: string }>(`/agents/${agentId}`, {
      method: 'DELETE',
    }),
  autodetectAgents: () =>
    apiFetch<AgentInfo[]>('/agents/autodetect', {
      method: 'POST',
    }),
  listSessions: () => apiFetch<SessionInfo[]>('/sessions'),
  createSession: (agentId: string, modelId: string, workspaceId: string) =>
    apiFetch<SessionInfo>('/sessions', {
      method: 'POST',
      body: JSON.stringify({ agentId, modelId, workspaceId }),
    }),
  patchSession: (
    sessionId: string,
    patch: { name?: string; agentId?: string; modelId?: string },
  ) =>
    apiFetch<{ status: string }>(`/sessions/${sessionId}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),
  sendPrompt: (sessionId: string, content: string) =>
    apiFetch<{ status: string }>(`/sessions/${sessionId}/prompt`, {
      method: 'POST',
      body: JSON.stringify({ content }),
    }),
  cancelSession: (sessionId: string) =>
    apiFetch<{ status: string }>(`/sessions/${sessionId}/cancel`, {
      method: 'POST',
    }),
  closeSession: (sessionId: string) =>
    apiFetch<{ status: string }>(`/sessions/${sessionId}`, {
      method: 'DELETE',
    }),

  // Pairing
  initiatePairing: (host: string, port: number) =>
    apiFetch<PairingSession>('/pair/initiate', {
      method: 'POST',
      body: JSON.stringify({ host, port }),
    }),
  verifyPasscode: (passcode: string, deviceName: string) =>
    apiFetch<DeviceCredential>('/pair/verify-passcode', {
      method: 'POST',
      body: JSON.stringify({ passcode, deviceName }),
    }),
  verifyToken: (token: string, deviceName: string) =>
    apiFetch<DeviceCredential>('/pair/verify-token', {
      method: 'POST',
      body: JSON.stringify({ token, deviceName }),
    }),

  // Devices
  listDevices: () => apiFetch<DeviceCredential[]>('/devices'),
  revokeDevice: (deviceId: string) =>
    apiFetch<{ status: string }>(`/devices/${deviceId}`, {
      method: 'DELETE',
    }),

  // Permissions
  getPendingPermissions: () =>
    apiFetch<PendingPermission[]>('/permissions/pending'),
  respondPermission: (requestId: string, decision: string) =>
    apiFetch<{ status: string }>(`/permissions/${requestId}/respond`, {
      method: 'POST',
      body: JSON.stringify({ decision }),
    }),
}
