import { useCallback, useState } from 'react'
import { getStoredTheme, setTheme as applyAndStore, type Theme } from '@/lib/theme'

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

  return { theme, setTheme }
}
