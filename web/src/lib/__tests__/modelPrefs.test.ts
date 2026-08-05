import { describe, it, expect, beforeEach, vi } from 'vitest'
import {
  groupModelsForSelect,
  pickDefaultModelId,
  getModelPrefs,
} from '@/lib/modelPrefs'
import type { ModelOption, ModelPrefs } from '@/lib/modelPrefs'

/** Build a ModelOption with a sane default shape. */
function mo(id: string, overrides: Partial<ModelOption> = {}): ModelOption {
  return { id, name: id, ...overrides }
}

/** Empty prefs convenience. */
const noPrefs: ModelPrefs = { pinned: [], recent: [] }

describe('groupModelsForSelect', () => {
  it('returns no groups when models is empty', () => {
    expect(groupModelsForSelect([], noPrefs)).toEqual([])
  })

  it('puts all models in a single "Models" group when there are no prefs', () => {
    const models = [mo('a'), mo('b'), mo('c')]
    const groups = groupModelsForSelect(models, noPrefs)
    expect(groups).toHaveLength(1)
    expect(groups[0]).toEqual({ id: 'all', label: 'Models', models })
  })

  it('places pinned first and the rest in "All models"', () => {
    const models = [mo('a'), mo('b'), mo('c'), mo('d')]
    const prefs: ModelPrefs = { pinned: ['b', 'd'], recent: [] }
    const groups = groupModelsForSelect(models, prefs)
    expect(groups.map((g) => g.id)).toEqual(['pinned', 'all'])
    expect(groups[0].models.map((m) => m.id)).toEqual(['b', 'd'])
    expect(groups[1].label).toBe('All models')
    expect(groups[1].models.map((m) => m.id)).toEqual(['a', 'c'])
  })

  it('surfaces all variants of a pinned base in the pinned group', () => {
    // Pinned is per-base: pinning "gpt-5.3-codex" pulls in every variant.
    const models = [
      mo('gpt-5.3-codex-low-fast'),
      mo('gpt-5.3-codex-high'),
      mo('other'),
    ]
    const prefs: ModelPrefs = { pinned: ['gpt-5.3-codex'], recent: [] }
    const groups = groupModelsForSelect(models, prefs)
    expect(groups.map((g) => g.id)).toEqual(['pinned', 'all'])
    expect(groups[0].models.map((m) => m.id)).toEqual([
      'gpt-5.3-codex-low-fast',
      'gpt-5.3-codex-high',
    ])
    expect(groups[1].models.map((m) => m.id)).toEqual(['other'])
  })

  it('emits a recent group deduped against pinned', () => {
    const models = [mo('a'), mo('b'), mo('c')]
    const prefs: ModelPrefs = { pinned: ['a'], recent: ['c', 'a', 'b'] }
    const groups = groupModelsForSelect(models, prefs)
    // 'a' is pinned, so it must not also appear in recent.
    const recent = groups.find((g) => g.id === 'recent')
    expect(recent?.models.map((m) => m.id)).toEqual(['c', 'b'])
  })

  it('emits a preferred group for models flagged preferred', () => {
    const models = [mo('a'), mo('b', { preferred: true }), mo('c')]
    const groups = groupModelsForSelect(models, noPrefs)
    const preferred = groups.find((g) => g.id === 'preferred')
    expect(preferred?.models.map((m) => m.id)).toEqual(['b'])
  })

  it('orders pinned → recent → preferred → all, each id once', () => {
    const models = [
      mo('fav'),
      mo('rec'),
      mo('pref', { preferred: true }),
      mo('other'),
    ]
    const prefs: ModelPrefs = { pinned: ['fav'], recent: ['rec'] }
    const groups = groupModelsForSelect(models, prefs)
    expect(groups.map((g) => g.id)).toEqual([
      'pinned',
      'recent',
      'preferred',
      'all',
    ])
    const ids = groups.flatMap((g) => g.models.map((m) => m.id))
    expect(ids).toEqual(['fav', 'rec', 'pref', 'other'])
    // No id appears more than once.
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('keeps a model in pinned only when it is also recent (first group wins)', () => {
    const models = [mo('a'), mo('b')]
    const prefs: ModelPrefs = { pinned: ['a'], recent: ['a', 'b'] }
    const groups = groupModelsForSelect(models, prefs)
    const pinnedIds = groups.find((g) => g.id === 'pinned')?.models.map((m) => m.id)
    const recentIds = groups.find((g) => g.id === 'recent')?.models.map((m) => m.id)
    expect(pinnedIds).toEqual(['a'])
    expect(recentIds).toEqual(['b']) // 'a' already used
  })

  it('surfaces a "Pinned" group when every model is pinned', () => {
    // Every model is pinned, so the "rest" bucket is empty. The pinned group
    // is non-empty, so the fallback (which only fires when groups is empty)
    // does not trigger — the single pinned group is the output.
    const models = [mo('a'), mo('b')]
    const prefs: ModelPrefs = { pinned: ['a', 'b'], recent: [] }
    const groups = groupModelsForSelect(models, prefs)
    expect(groups).toEqual([{ id: 'pinned', label: 'Pinned', models }])
  })
})

describe('pickDefaultModelId', () => {
  it('returns the stored id when it is still valid', () => {
    const models = [mo('a'), mo('b')]
    expect(pickDefaultModelId(models, 'b')).toBe('b')
  })

  it('falls back to the preferred model when the stored id is invalid', () => {
    const models = [mo('a'), mo('b', { preferred: true })]
    expect(pickDefaultModelId(models, 'gone')).toBe('b')
  })

  it('falls back to the first model when there is no stored id and no preferred', () => {
    const models = [mo('a'), mo('b')]
    expect(pickDefaultModelId(models, '')).toBe('a')
  })

  it('returns empty string when there are no models', () => {
    expect(pickDefaultModelId([], 'a')).toBe('')
    expect(pickDefaultModelId([], '')).toBe('')
  })

  it('treats an empty stored id as absent and falls through to preferred/first', () => {
    const models = [mo('a'), mo('b', { preferred: true })]
    expect(pickDefaultModelId(models, '')).toBe('b')
  })
})

describe('getModelPrefs migration', () => {
  beforeEach(() => {
    // Provide a fresh localStorage mock for each test.
    const store = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
      removeItem: (key: string) => void store.delete(key),
      clear: () => store.clear(),
      key: (i: number) => Array.from(store.keys())[i] ?? null,
      get length() {
        return store.size
      },
    })
  })

  it('returns empty prefs when nothing is stored', () => {
    expect(getModelPrefs('agent-1')).toEqual({ pinned: [], recent: [] })
  })

  it('migrates legacy favorites (per-variant) to pinned (per-base)', () => {
    // Legacy shape: favorites holds full variant ids.
    const legacy = {
      'agent-1': {
        favorites: ['gpt-5.3-codex-low-fast', 'gpt-5.3-codex-high'],
        recent: ['gpt-5.3-codex-low-fast'],
      },
    }
    localStorage.setItem('lai:modelPrefs', JSON.stringify(legacy))

    const prefs = getModelPrefs('agent-1')
    // Two variants of the same base collapse to one pinned base id.
    expect(prefs.pinned).toEqual(['gpt-5.3-codex'])
    expect(prefs.recent).toEqual(['gpt-5.3-codex-low-fast'])
  })

  it('dedupes multiple bases in legacy favorites preserving first-seen order', () => {
    const legacy = {
      'agent-1': {
        favorites: ['gpt-5.3-codex-low-fast', 'claude-opus-5-high', 'gpt-5.3-codex-max'],
        recent: [],
      },
    }
    localStorage.setItem('lai:modelPrefs', JSON.stringify(legacy))

    const prefs = getModelPrefs('agent-1')
    expect(prefs.pinned).toEqual(['gpt-5.3-codex', 'claude-opus-5'])
  })

  it('reads pinned directly when the new shape is already stored', () => {
    const next = {
      'agent-1': { pinned: ['gpt-5.3-codex'], recent: ['gpt-5.3-codex-low-fast'] },
    }
    localStorage.setItem('lai:modelPrefs', JSON.stringify(next))
    expect(getModelPrefs('agent-1')).toEqual({
      pinned: ['gpt-5.3-codex'],
      recent: ['gpt-5.3-codex-low-fast'],
    })
  })
})
