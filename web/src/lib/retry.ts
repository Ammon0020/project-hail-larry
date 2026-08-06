/**
 * Automatic retry with exponential backoff for transient REST API failures.
 *
 * Only WebSocket had automatic reconnection before this module. REST API
 * failures (send prompt, upload file, profile/model switch) required manual
 * retry — if the daemon was briefly unreachable during a send, the message
 * was lost and the user had to retype and resend. `withRetry` wraps mutating
 * API calls so transient failures (network errors, 502/503/504) retry
 * automatically with exponential backoff (1s, 2s, 4s), while non-transient
 * failures (4xx, 500) fail immediately without wasting retries.
 *
 * Read-only GET calls are deliberately NOT retried here — they are polled
 * regularly anyway, and retrying them would only mask connectivity issues
 * the WebSocket reconnect banner already surfaces.
 */

import { useEffect, useState } from 'react'
import { ApiError } from './api/client'

/** HTTP statuses that are transient and worth retrying. */
const TRANSIENT_STATUSES = new Set([502, 503, 504])

/**
 * Determines whether an error is transient and should be retried.
 *
 * Transient (retry):
 *   - Network errors — `TypeError: Failed to fetch` — the daemon is
 *     unreachable, DNS failed, or the connection was refused. These almost
 *     always resolve on retry once the daemon is back.
 *   - HTTP 502/503/504 — the backend is temporarily unavailable (restart,
 *     overload, upstream timeout).
 *
 * Non-transient (fail immediately):
 *   - Any 4xx (400, 401, 403, 404, 409, …) — the request itself is wrong;
 *     retrying an identical request cannot help.
 *   - HTTP 500 — a server-side bug / panic; retrying the same request is
 *     unlikely to help and can amplify load during an outage.
 *   - Any other 5xx not in the transient set.
 *
 * @param error The thrown error from the wrapped fetch call.
 * @returns `true` when the error is transient and a retry should be attempted.
 */
export function isTransientError(error: unknown): boolean {
  // Network errors (daemon unreachable, DNS failure, CORS, connection reset)
  // surface as `TypeError: Failed to fetch` from the browser's fetch layer.
  if (error instanceof TypeError) return true
  // ApiError carries the HTTP status; retry only the transient 5xx subset.
  if (error instanceof ApiError) return TRANSIENT_STATUSES.has(error.status)
  return false
}

/** Default maximum number of retries (not counting the initial attempt). */
export const DEFAULT_MAX_RETRIES = 3

/**
 * Default exponential backoff: 1s, 2s, 4s for attempts 0, 1, 2.
 *
 * @param attempt Zero-based retry attempt number (0 = first retry).
 * @returns Delay in milliseconds before the next attempt.
 */
export function defaultBackoff(attempt: number): number {
  return 1000 * 2 ** attempt
}

/** Default sleep implementation using `setTimeout`. */
function defaultSleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** Options for {@link withRetry}. */
export interface RetryOptions {
  /** Maximum retries after the initial attempt (default: 3). */
  maxRetries?: number
  /** Computes the delay before each retry attempt (default: exponential 1/2/4s). */
  getDelay?: (attempt: number) => number
  /** Notified before each retry's backoff sleep (useful for tests / logging). */
  onRetry?: (attempt: number, error: unknown) => void
  /** Injectable sleep for tests (default: `setTimeout`-based). */
  sleep?: (ms: number) => Promise<void>
}

/**
 * Wraps an async function with automatic retry on transient failures.
 *
 * Calls `fn` once; if it throws a transient error (see {@link isTransientError})
 * and the retry budget is not exhausted, waits the backoff delay and tries
 * again. Non-transient errors re-throw immediately. After exhausting all
 * retries the last error is re-thrown so the caller sees the real failure.
 *
 * During each backoff sleep the global retry indicator (see {@link useRetrying})
 * is armed so the UI can show a subtle "Retrying…" hint.
 *
 * @param fn      The async operation to attempt (typically a fetch call).
 * @param options Retry tuning (all optional; defaults: 3 retries, 1/2/4s backoff).
 * @returns The successful result of `fn`, or throws the last error.
 */
export async function withRetry<T>(fn: () => Promise<T>, options: RetryOptions = {}): Promise<T> {
  const maxRetries = options.maxRetries ?? DEFAULT_MAX_RETRIES
  const getDelay = options.getDelay ?? defaultBackoff
  const sleep = options.sleep ?? defaultSleep
  let lastError: unknown
  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      return await fn()
    } catch (error) {
      lastError = error
      // Stop on non-transient errors, or once the retry budget is exhausted.
      if (!isTransientError(error) || attempt >= maxRetries) {
        throw error
      }
      options.onRetry?.(attempt + 1, error)
      // Arm the UI indicator for the full backoff window so the user sees
      // feedback while we wait, then clear it before the next attempt.
      setRetryActive(true)
      try {
        await sleep(getDelay(attempt))
      } finally {
        setRetryActive(false)
      }
    }
  }
  // Unreachable — the loop either returns or throws — but keeps the type
  // checker happy and guards against a future logic change.
  throw lastError
}

// ---------------------------------------------------------------------------
// Global retry indicator — a tiny pub/sub so any component can observe whether
// a retry is in flight without threading state through every call site.
// ---------------------------------------------------------------------------

let retryActiveCount = 0
const retryListeners = new Set<(active: boolean) => void>()

/**
 * Increments or decrements the global retry counter and notifies subscribers.
 * A counter (not a boolean) correctly handles overlapping retries from
 * concurrent API calls.
 */
function setRetryActive(active: boolean): void {
  if (active) retryActiveCount++
  else retryActiveCount = Math.max(0, retryActiveCount - 1)
  const isRetry = retryActiveCount > 0
  for (const listener of retryListeners) listener(isRetry)
}

/**
 * React hook that subscribes to the global retry indicator.
 *
 * Returns `true` while at least one `withRetry` call is in a backoff sleep
 * before a retry attempt. Use this to show a subtle "Retrying…" hint in the
 * UI. The hook cleans up its subscription on unmount.
 *
 * @returns Whether a retry is currently in progress.
 */
export function useRetrying(): boolean {
  const [retrying, setRetrying] = useState(false)
  useEffect(() => {
    const listener = (active: boolean) => setRetrying(active)
    retryListeners.add(listener)
    // Sync to the current state on mount in case a retry started before
    // the component subscribed.
    setRetrying(retryActiveCount > 0)
    return () => {
      retryListeners.delete(listener)
    }
  }, [])
  return retrying
}
