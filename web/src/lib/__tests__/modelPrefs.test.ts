import { describe, it, expect } from 'vitest'
import { groupModelsForSelect, pickDefaultModelId } from '@/lib/modelPrefs'
import type { ModelOption, ModelPrefs } from '@/lib/modelPrefs'

/** Build a ModelOption with a sane default shape. */
function mo(id: string, overrides: Partial<ModelOption> = {}): ModelOption {
  return { id, name: id, ...overrides }
}

/** Empty prefs convenience. */
const noPrefs: ModelPrefs = { favorites: [], recent: [] }

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

  it('places favorites first and the rest in "All models"', () => {
    const models = [mo('a'), mo('b'), mo('c'), mo('d')]
    const prefs: ModelPrefs = { favorites: ['b', 'd'], recent: [] }
    const groups = groupModelsForSelect(models, prefs)
    expect(groups.map((g) => g.id)).toEqual(['favorites', 'all'])
    expect(groups[0].models.map((m) => m.id)).toEqual(['b', 'd'])
    expect(groups[1].label).toBe('All models')
    expect(groups[1].models.map((m) => m.id)).toEqual(['a', 'c'])
  })

  it('emits a recent group deduped against favorites', () => {
    const models = [mo('a'), mo('b'), mo('c')]
    const prefs: ModelPrefs = { favorites: ['a'], recent: ['c', 'a', 'b'] }
    const groups = groupModelsForSelect(models, prefs)
    // 'a' is a favorite, so it must not also appear in recent.
    const recent = groups.find((g) => g.id === 'recent')
    expect(recent?.models.map((m) => m.id)).toEqual(['c', 'b'])
  })

  it('emits a preferred group for models flagged preferred', () => {
    const models = [mo('a'), mo('b', { preferred: true }), mo('c')]
    const groups = groupModelsForSelect(models, noPrefs)
    const preferred = groups.find((g) => g.id === 'preferred')
    expect(preferred?.models.map((m) => m.id)).toEqual(['b'])
  })

  it('orders favorites → recent → preferred → all, each id once', () => {
    const models = [
      mo('fav'),
      mo('rec'),
      mo('pref', { preferred: true }),
      mo('other'),
    ]
    const prefs: ModelPrefs = { favorites: ['fav'], recent: ['rec'] }
    const groups = groupModelsForSelect(models, prefs)
    expect(groups.map((g) => g.id)).toEqual([
      'favorites',
      'recent',
      'preferred',
      'all',
    ])
    const ids = groups.flatMap((g) => g.models.map((m) => m.id))
    expect(ids).toEqual(['fav', 'rec', 'pref', 'other'])
    // No id appears more than once.
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('keeps a model in favorites only when it is also recent (first group wins)', () => {
    const models = [mo('a'), mo('b')]
    const prefs: ModelPrefs = { favorites: ['a'], recent: ['a', 'b'] }
    const groups = groupModelsForSelect(models, prefs)
    const favIds = groups.find((g) => g.id === 'favorites')?.models.map((m) => m.id)
    const recentIds = groups.find((g) => g.id === 'recent')?.models.map((m) => m.id)
    expect(favIds).toEqual(['a'])
    expect(recentIds).toEqual(['b']) // 'a' already used
  })

  it('surfaces a "Favorites" group when every model is a favorite', () => {
    // Every model is a favorite, so the "rest" bucket is empty. The favorites
    // group is non-empty, so the fallback (which only fires when groups is
    // empty) does not trigger — the single favorites group is the output.
    const models = [mo('a'), mo('b')]
    const prefs: ModelPrefs = { favorites: ['a', 'b'], recent: [] }
    const groups = groupModelsForSelect(models, prefs)
    expect(groups).toEqual([{ id: 'favorites', label: 'Favorites', models }])
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
