import { useEffect, useRef } from 'react'

/**
 * Registers one global capture-phase keydown handler for the lifetime of the
 * owner. The handler is stored in a ref so inline caller closures (which
 * change identity every render) don't churn the listener registry or drop
 * keypresses during the cleanup/re-subscribe window.
 */
export function useKeyboardShortcuts(handler: (event: KeyboardEvent) => void): void {
  const handlerRef = useRef(handler)
  useEffect(() => {
    handlerRef.current = handler
  }, [handler])
  useEffect(() => {
    const fn = (e: KeyboardEvent) => handlerRef.current(e)
    window.addEventListener('keydown', fn, true)
    return () => window.removeEventListener('keydown', fn, true)
  }, [])
}
