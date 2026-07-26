import { useCallback, useState } from 'react'

/**
 * Editor preferences persisted to `localStorage`. All keys are prefixed with
 * `lai:` so they group with the rest of the app's client-side state.
 *
 * `fontSize` and `wrap` are backward-compatible: existing users who stored
 * their preference under the legacy `lai:editor-font-size` key are migrated
 * transparently (read once on init, then written to the canonical `lai:fontSize`
 * key on the next change). `wrap` was previously in-memory only, so there is
 * nothing to migrate.
 */
export interface EditorSettings {
  /** Editor font size in px. Lifted so the StatusBar can read/adjust it. */
  fontSize: number
  /** Soft-wrap long lines instead of horizontal scrolling. */
  wrap: boolean
  /** Number of spaces per indent level (1/2/4/8). */
  tabSize: number
  /** Show line numbers in the gutter. */
  lineNumbers: boolean
  /** Show the fold gutter (collapsed on mobile regardless of this setting). */
  foldGutter: boolean
  /** Highlight the matching bracket pair around the cursor. */
  bracketMatching: boolean
  /** Re-indent lines as the user types (smart indent on Enter, etc.). */
  autoIndent: boolean
  /** Auto-close brackets and quotes. */
  closeBrackets: boolean
}

/** localStorage keys — exported so tests / migrations can reference them. */
const KEYS = {
  fontSize: 'lai:fontSize',
  /** Legacy key from before settings were consolidated; read for migration. */
  fontSizeLegacy: 'lai:editor-font-size',
  wrap: 'lai:wrap',
  tabSize: 'lai:tabSize',
  lineNumbers: 'lai:lineNumbers',
  foldGutter: 'lai:foldGutter',
  bracketMatching: 'lai:bracketMatching',
  autoIndent: 'lai:autoIndent',
  closeBrackets: 'lai:closeBrackets',
} as const

/** Reads a JSON-parsed localStorage value, falling back to `fallback`. */
function readJSON<T>(key: string, fallback: T): T {
  try {
    const stored = localStorage.getItem(key)
    if (stored === null) return fallback
    return JSON.parse(stored) as T
  } catch {
    return fallback
  }
}

/** Reads a numeric localStorage value (stored as a plain string or JSON). */
function readNumber(key: string, fallback: number): number {
  try {
    const stored = localStorage.getItem(key)
    if (stored === null) return fallback
    const parsed = JSON.parse(stored)
    return typeof parsed === 'number' && Number.isFinite(parsed) ? parsed : fallback
  } catch {
    return fallback
  }
}

/**
 * Builds the initial {@link EditorSettings}, honoring legacy keys so existing
 * users keep their preferences. `isDesktop` selects mobile-friendly defaults
 * (larger font, no fold gutter) when no stored value is present.
 */
function loadSettings(isDesktop: boolean): EditorSettings {
  // Font size: prefer the canonical key, then the legacy key, then default.
  const defaultFontSize = isDesktop ? 13 : 15
  let fontSize = readNumber(KEYS.fontSize, NaN)
  if (!Number.isFinite(fontSize)) {
    // Migrate from the legacy key used before settings were consolidated.
    const legacy = localStorage.getItem(KEYS.fontSizeLegacy)
    if (legacy) {
      const parsed = parseInt(legacy, 10)
      if (Number.isFinite(parsed)) fontSize = parsed
    }
  }
  if (!Number.isFinite(fontSize)) fontSize = defaultFontSize

  return {
    fontSize,
    wrap: readJSON<boolean>(KEYS.wrap, false),
    tabSize: readNumber(KEYS.tabSize, 2),
    lineNumbers: readJSON<boolean>(KEYS.lineNumbers, true),
    // Fold gutter is desktop-only by default (tiny targets on touch).
    foldGutter: readJSON<boolean>(KEYS.foldGutter, isDesktop),
    bracketMatching: readJSON<boolean>(KEYS.bracketMatching, true),
    autoIndent: readJSON<boolean>(KEYS.autoIndent, true),
    closeBrackets: readJSON<boolean>(KEYS.closeBrackets, true),
  }
}

/** Persists a single field to localStorage, swallowing write failures. */
function persist<K extends keyof EditorSettings>(key: K, value: EditorSettings[K]): void {
  try {
    localStorage.setItem(KEYS[key], JSON.stringify(value))
  } catch {
    // Ignore write failures (quota, disabled storage) — UI state still updates.
  }
}

/**
 * useEditorSettings — reads and updates editor preferences, persisting each
 * field to its own localStorage key.
 *
 * Returns the current {@link EditorSettings} plus an `update` function that
 * merges a partial patch (persisting only the changed keys) and convenience
 * setters (`setFontSize`, `setWrap`) that mirror the shapes previously used by
 * App.tsx so the StatusBar / TabBar controls can drop in unchanged.
 *
 * @param isDesktop Whether the viewport is desktop-sized. Only used to pick
 *   defaults for `fontSize` and `foldGutter` when no stored value exists.
 */
export function useEditorSettings(isDesktop: boolean): {
  settings: EditorSettings
  update: (patch: Partial<EditorSettings>) => void
  /** Font-size updater accepting a state-updater function (matches the
   *  StatusBar's `onFontSizeChange` prop shape). */
  setFontSize: (fn: (s: number) => number) => void
  /** Sets word-wrap on/off. */
  setWrap: (w: boolean) => void
} {
  const [settings, setSettings] = useState<EditorSettings>(() => loadSettings(isDesktop))

  const update = useCallback((patch: Partial<EditorSettings>) => {
    setSettings((prev) => {
      const next = { ...prev, ...patch }
      for (const key of Object.keys(patch) as (keyof EditorSettings)[]) {
        persist(key, next[key])
      }
      return next
    })
  }, [])

  const setFontSize = useCallback((fn: (s: number) => number) => {
    setSettings((prev) => {
      const next = Math.max(8, Math.min(32, fn(prev.fontSize)))
      persist('fontSize', next)
      return { ...prev, fontSize: next }
    })
  }, [])

  const setWrap = useCallback((w: boolean) => {
    setSettings((prev) => {
      persist('wrap', w)
      return { ...prev, wrap: w }
    })
  }, [])

  return { settings, update, setFontSize, setWrap }
}
