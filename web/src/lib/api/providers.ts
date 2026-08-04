/**
 * ACP provider management (session-scoped) endpoints.
 */

import { API_BASE, apiFetch, withAuthHeaders } from './client'

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
