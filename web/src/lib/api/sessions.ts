/**
 * Sessions, agents, events, prompt context settings, session history,
 * uploads, and export endpoints.
 */

import type { AppEvent, Attachment, Agent, PromptContextSettings, Session } from '@/types'
import { API_BASE, apiFetch, withAuthHeaders } from './client'

/** The user's current text selection in the editor, reported to the backend
 *  so the ACP prompt pipeline can send it as a resource block. Path is
 *  relative to the workspace root; startLine/endLine are 1-based and
 *  inclusive. An empty/undefined Text clears the selection. */
export interface EditorSelectionInfo {
  path: string
  startLine: number
  endLine: number
  text: string
}

/** Live agent-session history support. `available: false` means not warm. */
export interface SessionHistoryCapabilities {
  available: boolean
  canListSessions: boolean
  canLoadSession: boolean
  canResumeSession: boolean
  canCloseSession: boolean
  canDeleteSession: boolean
}

/** Response from POST /sessions/:id/uploads — a stored upload ready to attach. */
export interface UploadResult {
  id: string
  name: string
  mimeType: string
  url: string
  size: number
}

export function getSessionHistoryCapabilities(sessionId: string) {
  return apiFetch<SessionHistoryCapabilities>(`/sessions/${sessionId}/capabilities`)
}

// Events
export function getEvents(afterId = 0, limit = 100) {
  return apiFetch<AppEvent[]>(`/events?after=${afterId}&limit=${limit}`)
}

export function getSessionEvents(sessionId: string, afterId = 0, limit = 100) {
  return apiFetch<AppEvent[]>(`/events/${sessionId}?after=${afterId}&limit=${limit}`)
}

// Agents & Sessions
export function listAgents() {
  return apiFetch<Agent[]>('/agents')
}

export function addAgent(agent: Agent) {
  return apiFetch<Agent>('/agents', {
    method: 'POST',
    body: JSON.stringify(agent),
  })
}

export function deleteAgent(agentId: string) {
  return apiFetch<{ status: string }>(`/agents/${agentId}`, {
    method: 'DELETE',
  })
}

export function autodetectAgents() {
  return apiFetch<Agent[]>('/agents/autodetect', {
    method: 'POST',
  })
}

export function listSessions() {
  return apiFetch<Session[]>('/sessions')
}

export function createSession(
  agentId: string,
  modelId: string,
  workspaceId: string,
  profileId?: string,
) {
  return apiFetch<Session>('/sessions', {
    method: 'POST',
    body: JSON.stringify({ agentId, modelId, workspaceId, profileId }),
  })
}

export function patchSession(
  sessionId: string,
  patch: { name?: string; agentId?: string; modelId?: string; maxTransferBytes?: number },
) {
  return apiFetch<{ status: string }>(`/sessions/${sessionId}`, {
    method: 'PATCH',
    body: JSON.stringify(patch),
  })
}

export function reportSessionContext(
  sessionId: string,
  openFiles: string[],
  recentEdits: string[],
  selection?: EditorSelectionInfo,
) {
  return apiFetch<{ status: string }>(`/sessions/${sessionId}/context`, {
    method: 'POST',
    body: JSON.stringify({ openFiles, recentEdits, selection }),
  })
}

export function getPromptContextSettings() {
  return apiFetch<PromptContextSettings>('/settings/prompt-context')
}

export function putPromptContextSettings(settings: PromptContextSettings) {
  return apiFetch<PromptContextSettings>('/settings/prompt-context', {
    method: 'PUT',
    body: JSON.stringify(settings),
  })
}

export function sendPrompt(sessionId: string, content: string, attachments?: Attachment[]) {
  return apiFetch<{ status: string }>(`/sessions/${sessionId}/prompt`, {
    method: 'POST',
    body: JSON.stringify(
      attachments && attachments.length > 0
        ? { content, attachments }
        : { content },
    ),
  })
}

/** Uploads an image file via multipart/form-data. Uses `fetch` directly
 *  (not `apiFetch`) so the browser sets the multipart Content-Type and
 *  boundary automatically — `apiFetch` forces `application/json`. Mirrors
 *  `apiFetch`'s same-origin base URL and error handling. */
export async function uploadFile(sessionId: string, file: File): Promise<UploadResult> {
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
}

export function cancelSession(sessionId: string) {
  return apiFetch<{ status: string }>(`/sessions/${sessionId}/cancel`, {
    method: 'POST',
  })
}

export function closeSession(sessionId: string) {
  return apiFetch<{ status: string }>(`/sessions/${sessionId}`, {
    method: 'DELETE',
  })
}

/** Exports a conversation as a markdown transcript and triggers a browser
 *  download. Uses `fetch` directly (not `apiFetch`) because the response is
 *  a text/markdown blob, not JSON — `apiFetch` forces `application/json`
 *  and parses the body as JSON, which would corrupt the markdown. Mirrors
 *  `apiFetch`'s same-origin base URL and error handling. The download
 *  filename is taken from the Content-Disposition header the server sets,
 *  falling back to `session-<id>.md` when the header is absent. */
export async function exportSession(sessionId: string): Promise<void> {
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
}
