/**
 * MCP config and status endpoints.
 */

import { API_BASE, apiFetch, withAuthHeaders } from './client'

/** Health status of a single MCP server, returned by GET /api/mcp/status. */
export interface McpServerStatus {
  name: string
  enabled: boolean
  status: 'healthy' | 'unhealthy' | 'disabled' | 'unknown'
  error?: string
}

// ---- MCP config ----
// Kept on `api` like the rest of the REST surface. getMcpConfig uses fetch
// directly (not apiFetch) so the editor preserves exact mcp.json formatting
// on round-trips — apiFetch would re-parse and lose raw text.

/** GET /api/mcp — returns raw JSON text of mcp.json (or empty envelope). */
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

/** PUT /api/mcp — validates and writes raw JSON. Returns 400 on parse error. */
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

/** GET /api/mcp/status — on-demand health check of all configured MCP servers. */
export function getMcpStatus(): Promise<McpServerStatus[]> {
  return apiFetch<McpServerStatus[]>('/mcp/status')
}
