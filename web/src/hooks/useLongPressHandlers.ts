import { useCallback, useMemo, useRef } from 'react'

/**
 * Long-press (500ms) → open context menu; move/end/cancel clears the timer.
 * Owns the timer ref inside the hook so refs are never passed to a function
 * during render (react-hooks/refs).
 */
export function useLongPressHandlers(onOpen: () => void, enabled = true) {
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const clear = useCallback(() => {
    if (timer.current) {
      clearTimeout(timer.current)
      timer.current = null
    }
  }, [])
  const handlers = useMemo(
    () => ({
      onTouchStart: () => {
        if (!enabled) return
        clear()
        timer.current = setTimeout(onOpen, 500)
      },
      onTouchEnd: clear,
      onTouchMove: clear,
      onTouchCancel: clear,
    }),
    [onOpen, enabled, clear],
  )
  return { handlers, timer, clear }
}
