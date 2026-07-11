import { useCallback, useState } from 'react'

/**
 * Persisted state synced to `localStorage` under `key`.
 *
 * Reads the initial value from `localStorage` (JSON-parsed), falling back to
 * `fallback` when the key is missing or the stored value cannot be parsed.
 * Writes JSON-stringify the new value on every update. Write errors (e.g.
 * private mode, quota exceeded) are swallowed so a UI control never throws
 * just because persistence failed.
 *
 * The setter accepts either a plain value or an updater function, matching
 * `useState`'s setter shape so it can drop in anywhere `useState` is used.
 *
 * @param key      localStorage key to read/write.
 * @param fallback Value used when the key is missing or unparsable.
 */
export function useLocalStorage<T>(
  key: string,
  fallback: T,
): [T, (v: T | ((prev: T) => T)) => void] {
  const [value, setValue] = useState<T>(() => {
    try {
      const stored = localStorage.getItem(key)
      if (stored === null) return fallback
      return JSON.parse(stored) as T
    } catch {
      return fallback
    }
  })

  const set = useCallback(
    (v: T | ((prev: T) => T)) => {
      setValue((prev) => {
        const next = v instanceof Function ? v(prev) : v
        try {
          localStorage.setItem(key, JSON.stringify(next))
        } catch {
          // Ignore write failures (quota, disabled storage) — UI state still updates.
        }
        return next
      })
    },
    [key],
  )

  return [value, set]
}
