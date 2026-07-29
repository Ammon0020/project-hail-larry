/**
 * Theme management. Supports an explicit 'dark'/'light' choice plus 'system'
 * (follow the OS preference). The resolved theme is applied by toggling the
 * `.dark` class on <html>, which flips the CSS custom properties defined in
 * index.css. Defaults to 'dark' per Blueprint Sec 17.
 */
export type Theme = 'dark' | 'light' | 'system'

const STORAGE_KEY = 'lai:theme'
const VALID: readonly Theme[] = ['dark', 'light', 'system']

/** Reads the persisted theme preference, falling back to 'dark'. */
export function getStoredTheme(): Theme {
  try {
    const v = localStorage.getItem(STORAGE_KEY)
    return v && (VALID as readonly string[]).includes(v) ? (v as Theme) : 'dark'
  } catch {
    return 'dark'
  }
}

/** Returns true when the OS currently prefers a dark color scheme. */
function systemPrefersDark(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-color-scheme: dark)').matches
  )
}

/** Resolves a theme preference to the concrete 'dark' | 'light' to apply. */
export function resolveTheme(theme: Theme): 'dark' | 'light' {
  if (theme === 'system') return systemPrefersDark() ? 'dark' : 'light'
  return theme
}

/** Applies a resolved theme by toggling the `.dark` class on <html>. */
export function applyTheme(theme: Theme): void {
  const resolved = resolveTheme(theme)
  document.documentElement.classList.toggle('dark', resolved === 'dark')
}

/** Persists and applies a theme preference. */
export function setTheme(theme: Theme): void {
  try {
    localStorage.setItem(STORAGE_KEY, theme)
  } catch {
    // Quota / private mode — UI still works without persistence.
  }
  applyTheme(theme)
}

/**
 * Initializes theming on startup: applies the stored preference and, when it
 * is 'system', keeps it in sync with OS changes for the page lifetime. The
 * listener re-reads the stored preference on each OS change so an explicit
 * 'dark'/'light' choice made later (via setTheme) wins over the OS. Returns a
 * cleanup function for the media-query listener (a no-op when not following
 * the system).
 */
export function initTheme(): () => void {
  const stored = getStoredTheme()
  applyTheme(stored)
  if (stored !== 'system' || typeof window.matchMedia !== 'function') {
    return () => {}
  }
  const mq = window.matchMedia('(prefers-color-scheme: dark)')
  const onChange = () => {
    if (getStoredTheme() === 'system') applyTheme('system')
  }
  mq.addEventListener('change', onChange)
  return () => mq.removeEventListener('change', onChange)
}
