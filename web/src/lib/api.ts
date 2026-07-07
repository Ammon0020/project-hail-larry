/**
 * API client for the Local Agent Interface Go daemon.
 * All endpoints are relative to the same origin (served by the Go server).
 */

import type { Attachment } from '@/types'

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
  // Only safe to coerce when the caller expects an array (T extends unknown[]);
  // the double cast is intentional because `null` is not assignable to T.
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
  workspace?: string
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
  attachments?: Attachment[]
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

/** Options for a workspace content search (mirrors Go search.Options). */
export interface SearchOptions {
  pattern: string
  ignoreCase?: boolean
  maxResults?: number
  filePattern?: string
  contextLines?: number
}

/** A single search match within a file (mirrors Go search.Result). */
export interface SearchResult {
  path: string
  lineNumber: number
  lineContent: string
  matchStart: number
  matchEnd: number
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

/** Response from POST /sessions/:id/uploads — a stored upload ready to attach. */
export interface UploadResult {
  id: string
  name: string
  mimeType: string
  url: string
  size: number
}

// ---- API methods ----

export const api = {
  // Health — apiFetch prefixes /api, so this hits /api/health.
  health: () => apiFetch<{ status: string }>('/health'),

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
  searchWorkspace: (workspaceId: string, opts: SearchOptions) => {
    const params = new URLSearchParams()
    params.set('pattern', opts.pattern)
    if (opts.ignoreCase) params.set('ignoreCase', '1')
    if (opts.maxResults != null) params.set('maxResults', String(opts.maxResults))
    if (opts.filePattern) params.set('filePattern', opts.filePattern)
    if (opts.contextLines != null) params.set('contextLines', String(opts.contextLines))
    return apiFetch<SearchResult[]>(`/workspaces/${workspaceId}/search?${params.toString()}`)
  },

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
    patch: { name?: string; agentId?: string; modelId?: string; maxTransferBytes?: number },
  ) =>
    apiFetch<{ status: string }>(`/sessions/${sessionId}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }),
  reportSessionContext: (sessionId: string, openFiles: string[], recentEdits: string[]) =>
    apiFetch<{ status: string }>(`/sessions/${sessionId}/context`, {
      method: 'POST',
      body: JSON.stringify({ openFiles, recentEdits }),
    }),
  sendPrompt: (sessionId: string, content: string, attachments?: Attachment[]) =>
    apiFetch<{ status: string }>(`/sessions/${sessionId}/prompt`, {
      method: 'POST',
      body: JSON.stringify(
        attachments && attachments.length > 0
          ? { content, attachments }
          : { content },
      ),
    }),
  /** Uploads an image file via multipart/form-data. Uses `fetch` directly
   *  (not `apiFetch`) so the browser sets the multipart Content-Type and
   *  boundary automatically — `apiFetch` forces `application/json`. Mirrors
   *  `apiFetch`'s same-origin base URL and error handling. */
  uploadFile: async (sessionId: string, file: File): Promise<UploadResult> => {
    const form = new FormData()
    form.append('file', file)
    const res = await fetch(`${API_BASE}/sessions/${sessionId}/uploads`, {
      method: 'POST',
      body: form,
    })
    if (!res.ok) {
      const body = await res.json().catch(() => ({ error: res.statusText }))
      throw new Error(body.error || `HTTP ${res.status}`)
    }
    return (await res.json()) as UploadResult
  },
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
