/**
 * Profiles (S-PROF-REST + S-PROF-CHAT) endpoints.
 */

import { apiFetch } from './client'
import { withRetry } from '@/lib/retry'

/**
 * One named profile entry. Mirrors the Rust `ProfileEntry` struct returned by
 * `GET /api/profiles` and accepted by `PUT /api/profiles`.
 *
 * - `label`        — human-readable name (backend cap: 100 chars).
 * - `instructions` — system-prompt preamble (backend cap: 16 KiB).
 * - `mcpServers`   — optional complete-server allowlist. Omitted means all
 *                    enabled servers; `[]` means no MCP servers.
 * - `legacyTools`  — read-only migration data from the old tool whitelist.
 */
export interface ProfileEntry {
  label: string
  instructions: string
  mcpServers?: string[]
  legacyTools?: string[]
}

/**
 * Top-level profiles config. `defaultProfileId` must reference an existing
 * profile id; the backend rejects PUT with 400 if it dangles.
 */
export interface ProfileConfig {
  profiles: { [id: string]: ProfileEntry }
  defaultProfileId: string
}

/**
 * GET /api/profiles — returns the persisted profiles config, or the built-in
 * defaults (code/ask/plan) when no file exists yet. Throws `Error` carrying
 * the backend's `error` message on any non-2xx response.
 */
export async function getProfiles(): Promise<ProfileConfig> {
  return apiFetch<ProfileConfig>('/profiles')
}

/**
 * PUT /api/profiles — validates and persists the full profiles config.
 * Returns 200 on success; 400 (with an `error` body) on validation failure
 * (bad id, oversized label/instructions, dangling defaultProfileId). The
 * thrown `Error` carries the backend's message so the UI can surface it
 * inline without silently reverting local edits.
 */
export async function putProfiles(config: ProfileConfig): Promise<void> {
  await apiFetch<unknown>('/profiles', {
    method: 'PUT',
    body: JSON.stringify(config),
  })
}

/**
 * POST /sessions/:id/profile — switches the active profile for a live session.
 * The backend applies the profile's instructions to the next prompt. MCP
 * server access is fixed when the ACP session starts. Returns 200 on success, 404 when the session does not exist, and 400
 * when `profileId` is not a known profile id (both surface as thrown `Error`s).
 */
export async function setSessionProfile(sessionId: string, profileId: string): Promise<void> {
  // Retried: a transient failure during a profile switch would otherwise leave
  // the session on the old profile with no UI feedback. 4xx (unknown profile /
  // missing session) still fail immediately — only 502/503/504 and network
  // errors retry.
  await withRetry(() =>
    apiFetch<unknown>(`/sessions/${sessionId}/profile`, {
      method: 'POST',
      body: JSON.stringify({ profile: profileId }),
    }),
  )
}
