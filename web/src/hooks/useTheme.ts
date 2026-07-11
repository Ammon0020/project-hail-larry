import { useCallback, useEffect, useState } from 'react'
import { applyTheme, getStoredTheme, setTheme as applyAndStore, type Theme } from '@/lib/theme'

/**
 * useTheme — reads and updates the active theme preference. Changing the theme
 * persists it (localStorage) and applies the `.dark` class immediately via the
 * theme module, then updates local state so the UI reflects the new choice.
 */
export function useTheme(): { theme: Theme; setTheme: (t: Theme) => void } {
  const [theme, setThemeState] = useState<Theme>(() => getStoredTheme())

  const setTheme = useCallback((next: Theme) => {
    applyAndStore(next)
    setThemeState(next)
  }, [])

  // When following the system theme, subscribe to OS preference changes so
  // toggling dark/light at the OS level updates the app in real time.
  // initTheme only handles the startup case; this covers runtime switches.
  useEffect(() => {
    if (theme !== 'system' || typeof window.matchMedia !== 'function') return
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => applyTheme('system')
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [theme])

  return { theme, setTheme }
}
