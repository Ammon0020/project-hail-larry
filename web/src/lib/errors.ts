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

/**
 * The single user-facing wording for "this conversation is gone".
 *
 * Kept here rather than inline at each catch site so the phrasing cannot drift
 * between the send, upload, and profile paths — and so `isSessionNotFound`
 * keeps recognizing an error that has already been rewritten into this form.
 */
export const SESSION_GONE_MESSAGE =
  'This conversation is no longer available. Start a new chat.'

/**
 * Classify a caught value from a session-scoped API call.
 *
 * Every chat catch block needs the same two things: a displayable message from
 * an `unknown`, and whether the session itself has gone away (which calls for
 * resetting to the new-chat state rather than showing the raw text). Callers
 * keep their own recovery, which differs — restoring composer input, setting an
 * upload-specific error, reverting a dropdown.
 *
 * Args:
 *   err: The caught value. Non-`Error` throws fall back to `fallback`.
 *   fallback: Message to show when `err` carries no usable text.
 *
 * Returns:
 *   The message to display and whether the session no longer exists.
 */
export function describeSessionError(
  err: unknown,
  fallback: string,
): { message: string; sessionGone: boolean } {
  const message = err instanceof Error ? err.message : fallback
  return { message, sessionGone: isSessionNotFound(message) }
}
