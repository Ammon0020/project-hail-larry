/**
 * `localStorage` access that cannot throw.
 *
 * Browsers reject storage access outright — not just writes — when it is
 * disabled by policy or in some private-browsing modes, so a bare
 * `localStorage.getItem` can take down a whole render. Every call here is
 * guarded and degrades to "no stored value", which is the behavior each caller
 * already handles for a missing key.
 *
 * Failures are deliberately silent: these calls sit in render paths and hot
 * event handlers, and a console line per keystroke helps nobody. `set` returns
 * whether the write landed for the rare caller that wants to know.
 *
 * For React state that should persist, prefer `useLocalStorage`, which builds
 * on the same guarantees.
 */
export const safeStorage = {
  /** Stored string, or `null` when missing or storage is unavailable. */
  get(key: string): string | null {
    try {
      return localStorage.getItem(key)
    } catch {
      return null
    }
  },

  /** Write a string. Returns false when storage rejected it (quota, disabled). */
  set(key: string, value: string): boolean {
    try {
      localStorage.setItem(key, value)
      return true
    } catch {
      return false
    }
  },

  /** Remove a key. A failure is indistinguishable from it never existing. */
  remove(key: string): void {
    try {
      localStorage.removeItem(key)
    } catch {
      // Nothing to do — the value is unreachable either way.
    }
  },

  /** JSON-parsed value, or `fallback` when missing, unavailable, or corrupt. */
  getJson<T>(key: string, fallback: T): T {
    const raw = this.get(key)
    if (raw === null) return fallback
    try {
      return JSON.parse(raw) as T
    } catch {
      return fallback
    }
  },

  /** JSON-serialize and write. Returns false when the write was rejected. */
  setJson(key: string, value: unknown): boolean {
    try {
      return this.set(key, JSON.stringify(value))
    } catch {
      // Circular structures make stringify throw before storage is touched.
      return false
    }
  },

  /**
   * Numeric value, or `fallback` when missing, unavailable, or non-numeric.
   *
   * Blank text counts as non-numeric: `Number('')` is `0`, which would silently
   * turn a cleared panel width into a collapsed panel.
   */
  getNumber(key: string, fallback: number): number {
    const raw = this.get(key)
    if (raw === null || raw.trim() === '') return fallback
    const parsed = Number(raw)
    return Number.isFinite(parsed) ? parsed : fallback
  },
}
