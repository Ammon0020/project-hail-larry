/**
 * Shared client-side error helpers for matching backend / hook error text.
 */

/**
 * Returns true when an error message indicates the targeted session no longer
 * exists in the backend (404 "session not found: sess-…") or was already
 * rewritten by a hook into the friendly "no longer available" form.
 *
 * Used to recover from a stale activeSessionId (e.g. from localStorage) and to
 * reset the chat UI instead of showing a raw transport error.
 *
 * Args:
 *   message: Error text from a thrown Error, API response, or wrapper.
 *
 * Returns:
 *   True when the message matches known session-missing phrases.
 */
export function isSessionNotFound(message: string): boolean {
  const lower = message.toLowerCase()
  return (
    lower.includes('session not found') ||
    lower.includes('no longer available')
  )
}
