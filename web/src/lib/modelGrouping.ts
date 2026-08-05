/**
 * Model ID parsing and base-model grouping utilities.
 *
 * Agent model IDs encode a base model, an optional thinking level, and an
 * optional fast flag in a single string (e.g. "gpt-5.3-codex-high-fast").
 * This module derives structured data from those IDs for the model selector UI.
 */

import type { ModelOption } from '@/lib/modelPrefs'

/** Known thinking levels in display order (lowest → highest). */
export const THINKING_LEVELS = ['none', 'low', 'medium', 'high', 'xhigh', 'max'] as const
export type ThinkingLevel = (typeof THINKING_LEVELS)[number]

/** Set of known thinking levels for fast membership checks. */
const THINKING_LEVEL_SET: ReadonlySet<string> = new Set(THINKING_LEVELS)

/** Index of each thinking level for sort comparisons. */
const THINKING_LEVEL_INDEX: Record<string, number> = Object.fromEntries(
  THINKING_LEVELS.map((lvl, i) => [lvl, i]),
)

/** Parsed components of a model ID. */
export interface ParsedModelId {
  /** Base model ID (e.g. "gpt-5.3-codex" from "gpt-5.3-codex-high-fast"). */
  base: string
  /** Thinking level, or undefined if the model has no thinking variants. */
  thinking?: ThinkingLevel
  /** Whether this is a fast variant. */
  fast: boolean
}

/**
 * Parse a model ID into its base, thinking level, and fast components.
 * Tolerant of IDs with no thinking level or fast suffix.
 *
 * Strategy: strip the trailing `-fast` first, then check whether the remaining
 * string ends with `-<thinking-level>` for a known level. If so, that trailing
 * segment is the thinking level and everything before it is the base. This
 * handles base names that themselves contain "thinking" (e.g.
 * "claude-opus-5-thinking-high" → base "claude-opus-5-thinking", level "high").
 */
export function parseModelId(id: string): ParsedModelId {
  let fast = false
  let rest = id

  // Strip the trailing `-fast` suffix first; the fast flag is unambiguous.
  if (rest.endsWith('-fast')) {
    fast = true
    rest = rest.slice(0, -'-fast'.length)
  }

  // Look for a trailing `-<level>` where <level> is a known thinking level.
  const lastDash = rest.lastIndexOf('-')
  if (lastDash > 0) {
    const tail = rest.slice(lastDash + 1)
    if (THINKING_LEVEL_SET.has(tail)) {
      return {
        base: rest.slice(0, lastDash),
        thinking: tail as ThinkingLevel,
        fast,
      }
    }
  }

  return { base: rest, fast }
}

/**
 * Reconstruct a model ID from base, thinking, and fast components.
 * Returns the original ID shape by joining with hyphens.
 */
export function resolveModelId(
  base: string,
  thinking: ThinkingLevel | undefined,
  fast: boolean,
): string {
  const parts = [base]
  if (thinking) parts.push(thinking)
  if (fast) parts.push('fast')
  return parts.join('-')
}

/** Display name for a thinking level (title-cased, or "No Thinking" for none). */
export function thinkingLabel(level: ThinkingLevel): string {
  switch (level) {
    case 'none':
      return 'No Thinking'
    case 'xhigh':
      return 'Extra High'
    case 'low':
      return 'Low'
    case 'medium':
      return 'Medium'
    case 'high':
      return 'High'
    case 'max':
      return 'Max'
  }
}

/** A single variant within a base-model group. */
export interface ModelVariant {
  thinking?: ThinkingLevel
  fast: boolean
  modelId: string
  name: string
  preferred?: boolean
  supportsImages?: boolean
  description?: string
}

/** A group of model variants sharing the same base. */
export interface BaseModelGroup {
  baseId: string
  /** Display name for the group — derived from the first variant's name. */
  displayName: string
  /** Whether any variant is preferred. */
  preferred?: boolean
  variants: ModelVariant[]
}

/**
 * Derive a group display name from a variant's display name by stripping
 * trailing thinking-level and fast tokens. Falls back to the base id.
 *
 * Variant names are expected to be space-separated (e.g. "Codex 5.3 High Fast").
 */
function deriveDisplayName(variantName: string, baseId: string): string {
  const tokens = variantName.split(/\s+/).filter(Boolean)
  if (tokens.length === 0) return baseId

  const labelSet = new Set<string>(['No Thinking', 'Extra High', 'Fast'])
  // Single-word labels for the simple thinking levels.
  for (const lvl of ['Low', 'Medium', 'High', 'Max']) labelSet.add(lvl)

  // Strip trailing tokens that match a known label.
  while (tokens.length > 0 && labelSet.has(tokens[tokens.length - 1])) {
    tokens.pop()
  }

  return tokens.length > 0 ? tokens.join(' ') : baseId
}

/** Sort key for a variant: thinking level index (none sorts first), then fast. */
function variantSortKey(v: ModelVariant): [number, number] {
  const lvlIdx = v.thinking ? THINKING_LEVEL_INDEX[v.thinking] : -1
  return [lvlIdx, v.fast ? 1 : 0]
}

/**
 * Group a flat model list by base model ID.
 * Variants within each group are ordered by thinking level (ascending),
 * then non-fast before fast.
 */
export function groupModelsByBase(models: ModelOption[]): BaseModelGroup[] {
  const groups = new Map<string, BaseModelGroup>()

  for (const model of models) {
    const { base, thinking, fast } = parseModelId(model.id)
    let group = groups.get(base)
    if (!group) {
      group = {
        baseId: base,
        displayName: deriveDisplayName(model.name, base),
        variants: [],
      }
      groups.set(base, group)
    }

    const variant: ModelVariant = {
      thinking,
      fast,
      modelId: model.id,
      name: model.name,
      preferred: model.preferred,
      supportsImages: model.supportsImages,
      description: model.description,
    }
    group.variants.push(variant)

    if (model.preferred) group.preferred = true
  }

  // Sort variants within each group; pick display name from the first variant
  // after sorting so the canonical (lowest) variant drives the group label.
  for (const group of groups.values()) {
    group.variants.sort((a, b) => {
      const [la, fa] = variantSortKey(a)
      const [lb, fb] = variantSortKey(b)
      if (la !== lb) return la - lb
      return fa - fb
    })
    group.displayName = deriveDisplayName(group.variants[0].name, group.baseId)
  }

  return Array.from(groups.values()).sort((a, b) =>
    a.displayName.localeCompare(b.displayName),
  )
}
