import { useCallback, useEffect, useState } from 'react'
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

  // OS theme-following for 'system' is owned by initTheme (lib/theme.ts) for
  // the page lifetime; useTheme only needs to re-apply on explicit setTheme.
  return { theme, setTheme }
}

/**
 * useResolvedTheme — returns the concrete 'dark' | 'light' currently in effect,
 * re-rendering when the stored preference changes (via useTheme) or when the OS
 * preference flips while following 'system'. Used by surfaces that need a JS
 * theme value (e.g. picking a CodeMirror syntax theme), since CSS `data-theme`/
 * `dark:` alone cannot swap CodeMirror's highlight extensions.
 */
export function useResolvedTheme(): 'dark' | 'light' {
  const { theme } = useTheme()
  const [systemDark, setSystemDark] = useState<boolean>(
    () =>
      typeof window !== 'undefined' &&
      typeof window.matchMedia === 'function' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches,
  )

  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => setSystemDark(mq.matches)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])

  if (theme === 'system') return systemDark ? 'dark' : 'light'
  return theme
}
