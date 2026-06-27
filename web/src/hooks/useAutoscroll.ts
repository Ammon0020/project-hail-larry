import { useEffect, useRef, useState } from 'react'

/**
 * Threshold (in px) below which the user is considered "near the bottom".
 * If the distance from the current scroll position to the bottom is less
 * than this, autoscroll stays active and the jump-to-bottom button stays
 * hidden.
 */
const NEAR_BOTTOM_THRESHOLD = 80

/**
 * Smart autoscroll hook for a scrollable container.
 *
 * Auto-scrolls to the bottom as new content arrives, but only when the
 * user is already at (or near) the bottom. If the user has scrolled up
 * to read earlier content, the view stays put until they manually scroll
 * back down or click a "jump to bottom" affordance.
 *
 * @param containerRef Ref to the scrollable element.
 * @param deps Dependency array — the events/streaming content that should
 *   trigger an autoscroll check when it changes.
 * @returns `isAtBottom` (whether the user is currently near the bottom)
 *   and `scrollToBottom()` (imperatively snaps to the bottom).
 */
export function useAutoscroll<T extends HTMLElement>(
  containerRef: React.RefObject<T | null>,
  deps: unknown[],
): { isAtBottom: boolean; scrollToBottom: () => void } {
  const [isAtBottom, setIsAtBottom] = useState(true)
  // Tracks whether the user was near the bottom at the time new content
  // arrived. Kept in a ref so the scroll listener and the effect share a
  // single source of truth without re-subscribing listeners.
  const wasNearBottomRef = useRef(true)

  /** Returns true when the container is scrolled within the threshold of the bottom. */
  const computeAtBottom = (): boolean => {
    const el = containerRef.current
    if (!el) return true
    return el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM_THRESHOLD
  }

  /** Imperatively scrolls the container to the bottom and marks it at-bottom. */
  const scrollToBottom = () => {
    const el = containerRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
    wasNearBottomRef.current = true
    setIsAtBottom(true)
  }

  // Keep isAtBottom in sync as the user scrolls. This is the only place
  // setState is called from an event handler (not an effect), so it does
  // not run afoul of react-hooks/set-state-in-effect.
  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const handleScroll = () => {
      const atBottom = computeAtBottom()
      wasNearBottomRef.current = atBottom
      setIsAtBottom(atBottom)
    }
    el.addEventListener('scroll', handleScroll, { passive: true })
    // Sync initial state in case the container starts scrolled up.
    handleScroll()
    return () => el.removeEventListener('scroll', handleScroll)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- effect reads only containerRef + refs; computeAtBottom/scrollToBottom close over the stable ref
  }, [containerRef])

  // On new content (deps change), autoscroll only if the user was near the
  // bottom. Reading wasNearBottomRef (a ref) avoids setState-in-effect.
  // `deps` is caller-controlled; the effect body reads refs only (no
  // props/state), so a stale closure cannot drop a needed re-run.
  useEffect(() => {
    if (wasNearBottomRef.current) {
      scrollToBottom()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- deps is caller-controlled; effect reads refs only
  }, deps)

  return { isAtBottom, scrollToBottom }
}
