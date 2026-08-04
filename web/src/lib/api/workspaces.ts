/**
 * Workspace CRUD, file operations, search, preview, and health endpoints.
 */

import type { FileNode, SearchOptions, SearchResult } from '@/types'
import { apiFetch } from './client'

export interface WorkspaceInfo {
  id: string
  path: string
  name: string
  /** Per-workspace HTML preview trust state.
   *  - `null`/undefined = unknown (prompt on first HTML preview)
   *  - `true` = trusted (permissive CSP, cross-origin resources allowed)
   *  - `false` = untrusted (restrictive CSP, exfil blocked) */
  trusted?: boolean | null
}

// Health — apiFetch prefixes /api, so this hits /api/health.
export function health() {
  return apiFetch<{ status: string }>('/health')
}

// Workspaces
export function listWorkspaces() {
  return apiFetch<WorkspaceInfo[]>('/workspaces')
}

export function registerWorkspace(path: string) {
  return apiFetch<WorkspaceInfo>('/workspaces', {
    method: 'POST',
    body: JSON.stringify({ path }),
  })
}

export function getFileTree(workspaceId: string) {
  return apiFetch<FileNode[]>(`/workspaces/${workspaceId}/files`)
}

export function createPreviewSession(workspaceId: string) {
  return apiFetch<{ token: string; expiresInSeconds: number }>(
    `/workspaces/${workspaceId}/preview-session`,
    { method: 'POST' },
  )
}

/** PUT /api/workspaces/{id}/trust — sets the per-workspace HTML preview trust
 *  state. `null` = unknown (prompt), `true` = trusted, `false` = untrusted. */
export function setWorkspaceTrust(workspaceId: string, trusted: boolean | null | undefined) {
  return apiFetch<{ status: string }>(`/workspaces/${workspaceId}/trust`, {
    method: 'PUT',
    body: JSON.stringify({ trusted: trusted ?? null }),
  })
}

export function readFile(workspaceId: string, path: string) {
  return apiFetch<{ content: string; revision: number; path: string; isBinary?: boolean; previewable?: boolean }>(
    `/workspaces/${workspaceId}/file?path=${encodeURIComponent(path)}`,
  )
}

export function saveFile(workspaceId: string, path: string, content: string, expectedRevision: number) {
  return apiFetch<{ revision: number; path: string }>(`/workspaces/${workspaceId}/file`, {
    method: 'POST',
    body: JSON.stringify({ path, content, expectedRevision }),
  })
}

/** DELETE /api/workspaces/{id}/file?path= — removes a file (or empty folder). */
export function deleteFile(workspaceId: string, path: string) {
  return apiFetch<{ status: string }>(
    `/workspaces/${workspaceId}/file?path=${encodeURIComponent(path)}`,
    { method: 'DELETE' },
  )
}

/** POST /api/workspaces/{id}/rename — renames/moves a file or folder. */
export function renameFile(workspaceId: string, from: string, to: string) {
  return apiFetch<{ status: string }>(`/workspaces/${workspaceId}/rename`, {
    method: 'POST',
    body: JSON.stringify({ from, to }),
  })
}

/** POST /api/workspaces/{id}/mkdir — creates a directory (parents as needed). */
export function mkdir(workspaceId: string, path: string) {
  return apiFetch<{ status: string }>(`/workspaces/${workspaceId}/mkdir`, {
    method: 'POST',
    body: JSON.stringify({ path }),
  })
}

export function searchWorkspace(workspaceId: string, opts: SearchOptions) {
  const params = new URLSearchParams()
  params.set('pattern', opts.pattern)
  if (opts.ignoreCase) params.set('ignoreCase', '1')
  if (opts.maxResults != null) params.set('maxResults', String(opts.maxResults))
  if (opts.filePattern) params.set('filePattern', opts.filePattern)
  if (opts.contextLines != null) params.set('contextLines', String(opts.contextLines))
  return apiFetch<SearchResult[]>(`/workspaces/${workspaceId}/search?${params.toString()}`)
}
