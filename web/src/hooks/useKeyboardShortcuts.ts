import { useEffect } from 'react'

/**
 * Registers one global keydown handler for the current render. The capture-phase
 * listener is refreshed with the handler and removed when the owner unmounts.
 */
export function useKeyboardShortcuts(handler: (event: KeyboardEvent) => void): void {
  useEffect(() => {
    window.addEventListener('keydown', handler, true)
    return () => window.removeEventListener('keydown', handler, true)
  }, [handler])
}
