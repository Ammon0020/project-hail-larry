/**
 * API client for the Local Agent Interface Go daemon.
 * All endpoints are relative to the same origin (served by the Go server).
 */

import type { AppEvent, Attachment, Agent, Session, SearchOptions, SearchResult } from '@/types'

// Re-export types so existing callers importing them from '@/lib/api' keep
// working — the canonical definitions live in @/types.
export type { AppEvent, Agent, Session, SearchOptions, SearchResult }

const API_BASE = '/api'

/** localStorage key for the paired device credential (id + secret). */
const DEVICE_CREDENTIAL_KEY = 'lai:deviceCredential'

/**
 * Reads the paired device credential from localStorage.
 * Returns `{ id, secret }` or `null` when the device is not paired
 * (e.g. before completing the lock-screen passcode flow). The credential
 * is stored by LockScreen.tsx / useBackend.verifyPasscode after a successful
 * pairing handshake.
 */
export function getDeviceCredential(): { id: string; secret: string } | null {
  try {
    const raw = localStorage.getItem(DEVICE_CREDENTIAL_KEY)
    if (!raw) return null
    const cred = JSON.parse(raw) as { id?: string; secret?: string }
    if (!cred.id || !cred.secret) return null
    return { id: cred.id, secret: cred.secret }
  } catch {
    return null
  }
}

/**
 * Builds the `Authorization: Bearer <deviceId>:<secret>` header value for
 * the stored device credential, or `null` when no credential is stored.
 * The backend's `requireAuth` middleware checks this header (or query params
 * for WebSocket) on every non-pairing API route. Loopback connections bypass
 * auth, so the host browser works without it — but remote (LAN) devices
 * are rejected with 401 unless this header is present.
 */
function authHeader(): string | null {
  const cred = getDeviceCredential()
  if (!cred) return null
  return `Bearer ${cred.id}:${cred.secret}`
}

/**
 * Merges auth + caller headers into a single headers object. The auth header
 * is only added when a credential exists AND the caller hasn't already set
 * an Authorization header (e.g. for the pairing endpoints that run before
 * pairing completes — no credential exists yet, so this is a no-op there).
 */
function withAuthHeaders(custom?: HeadersInit): HeadersInit {
  const auth = authHeader()
  const headers: Record<string, string> = {}
  if (auth) headers['Authorization'] = auth
  if (custom) {
    // Flatten HeadersInit into a plain object so we can detect overrides.
    if (custom instanceof Headers) {
      custom.forEach((v, k) => { headers[k] = v })
    } else if (Array.isArray(custom)) {
      for (const [k, v] of custom) headers[k] = v
    } else {
      Object.assign(headers, custom)
    }
  }
  return headers
}

/** Generic fetch wrapper with JSON parsing. */
async function apiFetch<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...withAuthHeaders(options?.headers),
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

/** The user's current text selection in the editor, reported to the backend
 *  so the ACP prompt pipeline can send it as a resource block. Path is
 *  relative to the workspace root; startLine/endLine are 1-based and
 *  inclusive. An empty/undefined Text clears the selection. */
export interface EditorSelection {
  path: string
  startLine: number
  endLine: number
  text: string
}

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
    apiFetch<{ content: string; revision: number; path: string; isBinary?: boolean }>(
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
  listAgents: () => apiFetch<Agent[]>('/agents'),
  addAgent: (agent: Agent) =>
    apiFetch<Agent>('/agents', {
      method: 'POST',
      body: JSON.stringify(agent),
    }),
  deleteAgent: (agentId: string) =>
    apiFetch<{ status: string }>(`/agents/${agentId}`, {
      method: 'DELETE',
    }),
  autodetectAgents: () =>
    apiFetch<Agent[]>('/agents/autodetect', {
      method: 'POST',
    }),
  listSessions: () => apiFetch<Session[]>('/sessions'),
  createSession: (agentId: string, modelId: string, workspaceId: string) =>
    apiFetch<Session>('/sessions', {
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
  reportSessionContext: (
    sessionId: string,
    openFiles: string[],
    recentEdits: string[],
    selection?: EditorSelection,
  ) =>
    apiFetch<{ status: string }>(`/sessions/${sessionId}/context`, {
      method: 'POST',
      body: JSON.stringify({ openFiles, recentEdits, selection }),
    }),
  sendPrompt: (sessionId: string, content: string, attachments?: Attachment[], profile?: string) =>
    apiFetch<{ status: string }>(`/sessions/${sessionId}/prompt`, {
      method: 'POST',
      body: JSON.stringify(
        attachments && attachments.length > 0
          ? { content, attachments, ...(profile ? { profile } : {}) }
          : { content, ...(profile ? { profile } : {}) },
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
      headers: withAuthHeaders(),
      body: form,
    })
    if (!res.ok) {
      const body = (await res.json().catch(() => ({ error: res.statusText }))) as { error?: string }
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

  /** Exports a conversation as a markdown transcript and triggers a browser
   *  download. Uses `fetch` directly (not `apiFetch`) because the response is
   *  a text/markdown blob, not JSON — `apiFetch` forces `application/json`
   *  and parses the body as JSON, which would corrupt the markdown. Mirrors
   *  `apiFetch`'s same-origin base URL and error handling. The download
   *  filename is taken from the Content-Disposition header the server sets,
   *  falling back to `session-<id>.md` when the header is absent. */
  exportSession: async (sessionId: string): Promise<void> => {
    const res = await fetch(`${API_BASE}/sessions/${sessionId}/export`, {
      headers: withAuthHeaders(),
    })
    if (!res.ok) {
      const body = (await res.json().catch(() => ({ error: res.statusText }))) as { error?: string }
      throw new Error(body.error || `HTTP ${res.status}`)
    }
    const blob = await res.blob()
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    // Use the filename from Content-Disposition if available, otherwise
    // generate one. The server always sets the header, so the fallback is
    // defensive.
    const cd = res.headers.get('Content-Disposition')
    const match = cd?.match(/filename="?(.+?)"?$/)
    a.download = match?.[1] ?? `session-${sessionId}.md`
    a.click()
    URL.revokeObjectURL(url)
  },

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

// ---- MCP config ----

/** GET /api/mcp — returns raw JSON text of mcp.json (or empty envelope).
 *  Uses `fetch` directly (not `apiFetch`) because the body is returned
 *  verbatim so the editor preserves the user's exact formatting on
 *  round-trips — `apiFetch` would re-parse and lose the raw text. Mirrors
 *  `apiFetch`'s same-origin base URL and error handling. */
export async function getMcpConfig(): Promise<string> {
  const res = await fetch(`${API_BASE}/mcp`, {
    headers: withAuthHeaders(),
  })
  if (!res.ok) {
    const body = (await res.json().catch(() => ({ error: res.statusText }))) as { error?: string }
    throw new Error(body.error || `HTTP ${res.status}`)
  }
  return res.text()
}

/** PUT /api/mcp — validates and writes raw JSON. Returns 400 on parse error.
 *  The body is sent as-is (not re-stringified) so the user's formatting
 *  survives the round-trip; the backend parses it only to validate. */
export async function putMcpConfig(rawJson: string): Promise<void> {
  await apiFetch<unknown>('/mcp', {
    method: 'PUT',
    body: rawJson,
  })
}

/** PATCH /api/mcp/servers/{name} — toggles a single server's enabled flag. */
export async function patchMcpServer(name: string, enabled: boolean): Promise<void> {
  await apiFetch<unknown>(`/mcp/servers/${encodeURIComponent(name)}`, {
    method: 'PATCH',
    body: JSON.stringify({ enabled }),
  })
}

// ---- MCP health status ----

/** Health status of a single MCP server, returned by GET /api/mcp/status. */
export interface McpServerStatus {
  name: string
  enabled: boolean
  status: 'healthy' | 'unhealthy' | 'disabled' | 'unknown'
  error?: string
}

/** GET /api/mcp/status — on-demand health check of all configured MCP servers.
 *  Called when the McpPopout opens. */
export async function getMcpStatus(): Promise<McpServerStatus[]> {
  return apiFetch<McpServerStatus[]>('/mcp/status')
}

// ---- ACP provider management (session-scoped) ----

/**
 * A runtime-configurable provider exposed by an ACP agent for a session.
 * Mirrors the Go `providers.ProviderInfo` struct returned by
 * `GET /api/sessions/{id}/providers`.
 *
 * - `id`         — stable provider identifier (e.g. "anthropic", "modelrouter").
 * - `required`   — when true the provider cannot be disabled (the agent
 *                  rejects DELETE with 400).
 * - `supported`  — apiType protocols the agent accepts for this provider
 *                  (subset of: anthropic | openai | azure | vertex | bedrock).
 * - `current`    — the active configuration, or absent/null when the
 *                  provider is disabled.
 */
export interface ProviderInfo {
  id: string
  required: boolean
  supported: string[]
  current?: { apiType: string; baseUrl: string } | null
}

/**
 * Sentinel error thrown by {@link listProviders} when the agent does not
 * support runtime provider configuration (HTTP 501). Callers can branch on
 * `instanceof UnsupportedProvidersError` to render a "not supported" notice
 * without conflating it with a transport/auth failure.
 */
export class UnsupportedProvidersError extends Error {
  constructor(message = 'Agent does not support runtime provider configuration') {
    super(message)
    this.name = 'UnsupportedProvidersError'
  }
}

/**
 * GET /api/sessions/{id}/providers — lists the runtime-configurable
 * providers for the session's agent.
 *
 * Throws {@link UnsupportedProvidersError} when the agent returns 501
 * (no provider support). Any other non-2xx response is rethrown as a
 * plain `Error` carrying the backend's `error` message.
 */
export async function listProviders(sessionId: string): Promise<ProviderInfo[]> {
  const res = await fetch(`${API_BASE}/sessions/${sessionId}/providers`, {
    headers: withAuthHeaders(),
  })
  if (res.status === 501) {
    throw new UnsupportedProvidersError()
  }
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new Error(body.error || `HTTP ${res.status}`)
  }
  const data = await res.json()
  // Go encodes nil slices as null — coerce to [] for callers expecting an array.
  return (data ?? []) as ProviderInfo[]
}

/**
 * PUT /api/sessions/{id}/providers/{providerId} — sets or updates a
 * provider's apiType, baseUrl, and optional headers for the session.
 *
 * `headers` may carry auth tokens; they are sent over the existing
 * authenticated channel and never logged by this client. Returns 200 on
 * success; the backend returns 400 for a bad apiType/baseUrl or 501 when
 * the agent lacks provider support (both surface as thrown `Error`s).
 */
export async function setProvider(
  sessionId: string,
  providerId: string,
  apiType: string,
  baseUrl: string,
  headers?: Record<string, string>,
): Promise<void> {
  await apiFetch<unknown>(
    `/sessions/${sessionId}/providers/${encodeURIComponent(providerId)}`,
    {
      method: 'PUT',
      body: JSON.stringify({ apiType, baseUrl, headers: headers ?? {} }),
    },
  )
}

/**
 * DELETE /api/sessions/{id}/providers/{providerId} — disables a provider
 * for the session (clears its `current` config). The backend returns 400
 * when the provider is marked `required` (those cannot be disabled);
 * that surfaces as a thrown `Error`.
 */
export async function disableProvider(sessionId: string, providerId: string): Promise<void> {
  await apiFetch<unknown>(
    `/sessions/${sessionId}/providers/${encodeURIComponent(providerId)}`,
    { method: 'DELETE' },
  )
}
