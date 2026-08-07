import { describe, it, expect, beforeEach, vi } from 'vitest'
import { safeStorage } from '../safeStorage'

// Tests run in node, not jsdom, so localStorage is stubbed per test — same
// approach as modelPrefs.test.ts.
beforeEach(() => {
  const store = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
    removeItem: (key: string) => void store.delete(key),
    clear: () => store.clear(),
  })
})

/** Replace localStorage with one that rejects every operation. */
function breakStorage() {
  const deny = () => {
    throw new Error('SecurityError: storage is disabled')
  }
  vi.stubGlobal('localStorage', {
    getItem: deny,
    setItem: deny,
    removeItem: deny,
    clear: deny,
  })
}

describe('safeStorage', () => {
  it('round-trips a value through the real store', () => {
    safeStorage.set('k', 'v')
    expect(safeStorage.get('k')).toBe('v')
    safeStorage.remove('k')
    expect(safeStorage.get('k')).toBeNull()
  })

  it('returns null for a missing key, matching localStorage.getItem', () => {
    expect(safeStorage.get('never-written')).toBeNull()
  })

  // Storage access throws outright when disabled by policy or in some private
  // browsing modes. Unguarded call sites crash the whole render; these must not.
  it('never throws when storage is unavailable', () => {
    breakStorage()

    expect(() => safeStorage.set('k', 'v')).not.toThrow()
    expect(() => safeStorage.remove('k')).not.toThrow()
    expect(safeStorage.get('k')).toBeNull()
  })

  it('reports write failure so callers can react if they care', () => {
    expect(safeStorage.set('k', 'v')).toBe(true)
    breakStorage()
    expect(safeStorage.set('k', 'v')).toBe(false)
  })

  it('parses JSON, falling back when the stored text is corrupt', () => {
    safeStorage.set('j', JSON.stringify({ a: 1 }))
    expect(safeStorage.getJson('j', { a: 0 })).toEqual({ a: 1 })

    safeStorage.set('j', '{not json')
    expect(safeStorage.getJson('j', { a: 0 })).toEqual({ a: 0 })

    expect(safeStorage.getJson('absent', { a: 0 })).toEqual({ a: 0 })
  })

  it('serializes JSON on write', () => {
    safeStorage.setJson('j', [1, 2])
    expect(safeStorage.get('j')).toBe('[1,2]')
  })

  it('does not swallow a JSON value that is legitimately null', () => {
    safeStorage.setJson('j', null)
    expect(safeStorage.getJson<number | null>('j', 5)).toBeNull()
  })

  it('reads numbers, rejecting absent and non-numeric text', () => {
    safeStorage.set('n', '42')
    expect(safeStorage.getNumber('n', 0)).toBe(42)

    safeStorage.set('n', 'abc')
    expect(safeStorage.getNumber('n', 7)).toBe(7)

    expect(safeStorage.getNumber('absent', 7)).toBe(7)
  })

  it('treats empty string as non-numeric rather than zero', () => {
    // Number('') is 0, which would silently collapse a cleared panel width to
    // zero and make the panel vanish.
    safeStorage.set('n', '')
    expect(safeStorage.getNumber('n', 300)).toBe(300)
  })

  it('is silent about failures rather than logging on every call', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    breakStorage()

    safeStorage.get('k')
    safeStorage.set('k', 'v')

    expect(spy).not.toHaveBeenCalled()
    spy.mockRestore()
  })
})
