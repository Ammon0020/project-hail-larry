/**
 * Shared API client infrastructure: base URL, device credential storage,
 * auth headers, the generic fetch wrapper, and the API error type.
 */

export const API_BASE = '/api'

/** sessionStorage key for the paired device credential (id + secret).
 *  sessionStorage (not localStorage) limits exposure to the tab session —
 *  the secret is cleared on tab close rather than persisting indefinitely.
 *  TODO: migrate to an HttpOnly; SameSite=Strict; Secure cookie set by the
 *  backend on POST /api/pair/verify-passcode so XSS cannot read it at all. */
const DEVICE_CREDENTIAL_KEY = 'lai:deviceCredential'

/**
 * Reads the paired device credential from sessionStorage.
 * Returns `{ id, secret }` or `null` when the device is not paired
 * (e.g. before completing the lock-screen passcode flow). The credential
 * is stored by LockScreen.tsx / useBackend.verifyPasscode after a successful
 * pairing handshake.
 */
export function getDeviceCredential(): { id: string; secret: string } | null {
  try {
    const raw = sessionStorage.getItem(DEVICE_CREDENTIAL_KEY)
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
 * Builds a URL for the browse-preview endpoint (GET /preview/{id}/{path}).
 * Top-level `/preview/...` (not under `/api`) so relative asset URLs in the
 * iframe resolve correctly. A one-time ticket bootstraps an HttpOnly preview
 * cookie, allowing relative assets to load without exposing device credentials.
 * Used by both BrowsePreview (multi-file static sites) and FileViewer (single
 * binary/media files) — the preview endpoint serves the same raw bytes as
 * /api/workspaces/{id}/raw but authenticates via the cookie instead of
 * putting the long-lived device secret in the URL query string.
 */
export function previewFileUrl(workspaceId: string, entryPath: string, previewToken?: string): string {
  const segments = entryPath
    .split(/[/\\]+/)
    .filter(Boolean)
    .map(encodeURIComponent)
    .join('/')
  const qs = previewToken ? `?previewToken=${encodeURIComponent(previewToken)}` : ''
  return `/preview/${encodeURIComponent(workspaceId)}/${segments}${qs}`
}

/**
 * Merges auth + caller headers into a single headers object. The auth header
 * is only added when a credential exists AND the caller hasn't already set
 * an Authorization header (e.g. for the pairing endpoints that run before
 * pairing completes — no credential exists yet, so this is a no-op there).
 */
export function withAuthHeaders(custom?: HeadersInit): HeadersInit {
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
export async function apiFetch<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...withAuthHeaders(options?.headers),
    },
  })
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new ApiError(body.error || `HTTP ${res.status}`, res.status)
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

/** API errors retain their HTTP status so callers can give conflict-specific guidance. */
export class ApiError extends Error {
  readonly status: number

  constructor(message: string, status: number) {
    super(message)
    this.status = status
  }
}
