import { describe, expect, it } from 'vitest'

import type { ModelOption } from '@/lib/modelPrefs'
import {
  groupModelsByBase,
  parseModelId,
  resolveModelId,
  thinkingLabel,
  THINKING_LEVELS,
  type ThinkingLevel,
} from '@/lib/modelGrouping'

describe('parseModelId', () => {
  it.each([
    ['gpt-5.3-codex-low', { base: 'gpt-5.3-codex', thinking: 'low', fast: false }],
    ['gpt-5.3-codex-low-fast', { base: 'gpt-5.3-codex', thinking: 'low', fast: true }],
    ['gpt-5.3-codex', { base: 'gpt-5.3-codex', thinking: undefined, fast: false }],
    ['gpt-5.3-codex-high-fast', { base: 'gpt-5.3-codex', thinking: 'high', fast: true }],
    // "thinking" is part of the base name; "high" is the level.
    ['claude-opus-5-thinking-high', { base: 'claude-opus-5-thinking', thinking: 'high', fast: false }],
    ['claude-opus-5-high', { base: 'claude-opus-5', thinking: 'high', fast: false }],
    ['claude-opus-5-high-fast', { base: 'claude-opus-5', thinking: 'high', fast: true }],
    ['claude-opus-5-low', { base: 'claude-opus-5', thinking: 'low', fast: false }],
    ['auto', { base: 'auto', thinking: undefined, fast: false }],
    ['composer-2.5', { base: 'composer-2.5', thinking: undefined, fast: false }],
    ['swe-1-7-medium', { base: 'swe-1-7', thinking: 'medium', fast: false }],
    ['gpt-5.6-sol-none', { base: 'gpt-5.6-sol', thinking: 'none', fast: false }],
    ['gpt-5.6-sol-xhigh-fast', { base: 'gpt-5.6-sol', thinking: 'xhigh', fast: true }],
    ['claude-fable-5-thinking-xhigh', { base: 'claude-fable-5-thinking', thinking: 'xhigh', fast: false }],
    ['claude-sonnet-5-thinking-high', { base: 'claude-sonnet-5-thinking', thinking: 'high', fast: false }],
    ['cursor-grok-4.5-high', { base: 'cursor-grok-4.5', thinking: 'high', fast: false }],
    ['cursor-grok-4.5-high-fast', { base: 'cursor-grok-4.5', thinking: 'high', fast: true }],
    ['kimi-k3-high', { base: 'kimi-k3', thinking: 'high', fast: false }],
  ])('parseModelId(%j) → %j', (input, expected) => {
    expect(parseModelId(input)).toEqual(expected)
  })

  it('handles a bare "fast" suffix with no thinking level', () => {
    expect(parseModelId('foo-fast')).toEqual({ base: 'foo', thinking: undefined, fast: true })
  })

  it('does not misinterpret "fast" as a thinking level', () => {
    // "fast" is not in THINKING_LEVELS, so it stays part of the base.
    expect(parseModelId('foo-bar')).toEqual({ base: 'foo-bar', thinking: undefined, fast: false })
  })
})

describe('resolveModelId', () => {
  it.each<[string, ThinkingLevel | undefined, boolean, string]>([
    ['gpt-5.3-codex', 'low', false, 'gpt-5.3-codex-low'],
    ['gpt-5.3-codex', 'low', true, 'gpt-5.3-codex-low-fast'],
    ['gpt-5.3-codex', undefined, false, 'gpt-5.3-codex'],
    ['gpt-5.3-codex', undefined, true, 'gpt-5.3-codex-fast'],
    ['auto', undefined, false, 'auto'],
  ])('resolveModelId(%j, %j, %j) → %j', (base, thinking, fast, expected) => {
    expect(resolveModelId(base, thinking, fast)).toBe(expected)
  })

  it('round-trips through parseModelId for all known examples', () => {
    const ids = [
      'gpt-5.3-codex-low',
      'gpt-5.3-codex-low-fast',
      'gpt-5.3-codex',
      'gpt-5.3-codex-high-fast',
      'claude-opus-5-thinking-high',
      'claude-opus-5-high',
      'claude-opus-5-high-fast',
      'claude-opus-5-low',
      'auto',
      'composer-2.5',
      'swe-1-7-medium',
      'gpt-5.6-sol-none',
      'gpt-5.6-sol-xhigh-fast',
      'claude-fable-5-thinking-xhigh',
      'claude-sonnet-5-thinking-high',
      'cursor-grok-4.5-high',
      'cursor-grok-4.5-high-fast',
      'kimi-k3-high',
    ]
    for (const id of ids) {
      const { base, thinking, fast } = parseModelId(id)
      expect(resolveModelId(base, thinking, fast)).toBe(id)
    }
  })
})

describe('thinkingLabel', () => {
  it('labels every known level', () => {
    const expected: Record<string, string> = {
      none: 'No Thinking',
      low: 'Low',
      medium: 'Medium',
      high: 'High',
      xhigh: 'Extra High',
      max: 'Max',
    }
    for (const lvl of THINKING_LEVELS) {
      expect(thinkingLabel(lvl)).toBe(expected[lvl])
    }
  })
})

describe('groupModelsByBase', () => {
  const models: ModelOption[] = [
    { id: 'gpt-5.3-codex-high-fast', name: 'Codex 5.3 High Fast' },
    { id: 'gpt-5.3-codex-low', name: 'Codex 5.3 Low' },
    { id: 'gpt-5.3-codex-low-fast', name: 'Codex 5.3 Low Fast' },
    { id: 'gpt-5.3-codex', name: 'Codex 5.3' },
    { id: 'claude-opus-5-thinking-high', name: 'Claude Opus 5 Thinking High' },
    { id: 'claude-opus-5-high', name: 'Claude Opus 5 High' },
    { id: 'auto', name: 'Auto', preferred: true },
  ]

  it('groups by base id', () => {
    const groups = groupModelsByBase(models)
    const baseIds = groups.map((g) => g.baseId).sort()
    expect(baseIds).toEqual(
      ['auto', 'claude-opus-5', 'claude-opus-5-thinking', 'gpt-5.3-codex'].sort(),
    )
  })

  it('sorts variants by thinking level then non-fast before fast', () => {
    const groups = groupModelsByBase(models)
    const codex = groups.find((g) => g.baseId === 'gpt-5.3-codex')!
    expect(codex.variants.map((v) => v.modelId)).toEqual([
      'gpt-5.3-codex',
      'gpt-5.3-codex-low',
      'gpt-5.3-codex-low-fast',
      'gpt-5.3-codex-high-fast',
    ])
  })

  it('derives display name by stripping thinking/fast tokens', () => {
    const groups = groupModelsByBase(models)
    const byBase = new Map(groups.map((g) => [g.baseId, g.displayName]))
    expect(byBase.get('gpt-5.3-codex')).toBe('Codex 5.3')
    expect(byBase.get('claude-opus-5')).toBe('Claude Opus 5')
    expect(byBase.get('claude-opus-5-thinking')).toBe('Claude Opus 5 Thinking')
    expect(byBase.get('auto')).toBe('Auto')
  })

  it('marks a group preferred if any variant is preferred', () => {
    const groups = groupModelsByBase(models)
    const auto = groups.find((g) => g.baseId === 'auto')!
    expect(auto.preferred).toBe(true)
    const codex = groups.find((g) => g.baseId === 'gpt-5.3-codex')!
    expect(codex.preferred).toBeUndefined()
  })

  it('sorts groups alphabetically by display name', () => {
    const groups = groupModelsByBase(models)
    const names = groups.map((g) => g.displayName)
    const sorted = [...names].sort((a, b) => a.localeCompare(b))
    expect(names).toEqual(sorted)
  })

  it('preserves variant metadata (preferred, supportsImages, description)', () => {
    const modelsWithMeta: ModelOption[] = [
      {
        id: 'kimi-k3-high',
        name: 'Kimi K3 High',
        preferred: true,
        supportsImages: true,
        description: 'kimi desc',
      },
    ]
    const groups = groupModelsByBase(modelsWithMeta)
    const v = groups[0].variants[0]
    expect(v).toMatchObject({
      thinking: 'high',
      fast: false,
      modelId: 'kimi-k3-high',
      name: 'Kimi K3 High',
      preferred: true,
      supportsImages: true,
      description: 'kimi desc',
    })
  })

  it('returns an empty array for empty input', () => {
    expect(groupModelsByBase([])).toEqual([])
  })
})
