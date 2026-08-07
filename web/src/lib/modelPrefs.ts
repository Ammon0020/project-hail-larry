/**
 * Per-agent model preferences stored in localStorage.
 *
 * - pinned: user-pinned base model ids (order preserved). A pinned base means
 *   every variant of that base model is surfaced in the "Pinned" group.
 * - recent: most-recently-used full variant ids (newest first). Recent is kept
 *   as the exact variant the user last selected.
 *
 * Specific model catalogs churn across agents; this module never hardcodes
 * model ids — it only tracks ids the user actually selected.
 */

import { parseModelId } from '@/lib/modelGrouping'
import { safeStorage } from './safeStorage'

export interface ModelPrefs {
  /** Base model ids (e.g. "gpt-5.3-codex"), order preserved. */
  pinned: string[]
  /** Full variant ids (e.g. "gpt-5.3-codex-low-fast"), newest first. */
  recent: string[]
}

const STORAGE_KEY = 'lai:modelPrefs'
const MAX_RECENT = 8
const MAX_PINNED = 20

/**
 * Raw stored shape. We accept legacy entries that still carry `favorites`
 * (per-variant ids) so we can migrate them on read. `favorites` is preserved
 * in storage for backward compat but never read after migration.
 */
interface StoredPrefs {
  pinned?: string[]
  favorites?: string[]
  recent?: string[]
}

type StoredByAgent = Record<string, StoredPrefs>

function emptyPrefs(): ModelPrefs {
  return { pinned: [], recent: [] }
}

function readAll(): StoredByAgent {
  try {
    const raw = safeStorage.get(STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as StoredByAgent
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

function writeAll(all: StoredByAgent): void {
  try {
    safeStorage.setJson(STORAGE_KEY, all)
  } catch {
    // Quota / private mode — UI still works without persistence.
  }
}

/**
 * Migrate a legacy `favorites` array (per-variant ids) to `pinned` (base ids).
 * Deduplicates: multiple variants of the same base collapse to one entry.
 * Order is preserved by first occurrence of each base.
 */
function migrateFavoritesToFavorites(favorites: string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const id of favorites) {
    if (!id) continue
    const base = parseModelId(id).base
    if (!base || seen.has(base)) continue
    seen.add(base)
    out.push(base)
  }
  return out
}

/** Load prefs for one agent harness id, migrating legacy `favorites` on read. */
export function getModelPrefs(agentId: string): ModelPrefs {
  if (!agentId) return emptyPrefs()
  const entry = readAll()[agentId]
  if (!entry) return emptyPrefs()

  const pinned = Array.isArray(entry.pinned)
    ? entry.pinned.filter(Boolean)
    : Array.isArray(entry.favorites)
      ? migrateFavoritesToFavorites(entry.favorites)
      : []

  // If we migrated from favorites, persist the new shape (keep favorites for
  // backward compat in case other code still reads it).
  if (!Array.isArray(entry.pinned) && Array.isArray(entry.favorites) && pinned.length) {
    const all = readAll()
    all[agentId] = { ...entry, pinned, favorites: entry.favorites }
    writeAll(all)
  }

  return {
    pinned,
    recent: Array.isArray(entry.recent) ? entry.recent.filter(Boolean) : [],
  }
}

function saveAgent(agentId: string, prefs: ModelPrefs): void {
  if (!agentId) return
  const all = readAll()
  all[agentId] = prefs
  writeAll(all)
}

/** Record a model selection as most-recent for this agent. */
export function pushRecentModel(agentId: string, modelId: string): ModelPrefs {
  const prefs = getModelPrefs(agentId)
  if (!modelId) return prefs
  const recent = [modelId, ...prefs.recent.filter((id) => id !== modelId)].slice(0, MAX_RECENT)
  const next = { ...prefs, recent }
  saveAgent(agentId, next)
  return next
}

/**
 * Toggle a base model id in the pinned list. Returns the updated prefs.
 * Caller is expected to pass a base id (e.g. "gpt-5.3-codex"); passing a full
 * variant id will still work but is discouraged — only the exact string match
 * is toggled.
 */
export function togglePinnedModel(agentId: string, baseModelId: string): ModelPrefs {
  const prefs = getModelPrefs(agentId)
  if (!baseModelId) return prefs
  const isPinned = prefs.pinned.includes(baseModelId)
  const pinned = isPinned
    ? prefs.pinned.filter((id) => id !== baseModelId)
    : [...prefs.pinned, baseModelId].slice(0, MAX_PINNED)
  const next = { ...prefs, pinned }
  saveAgent(agentId, next)
  return next
}

export interface ModelOption {
  id: string
  name: string
  preferred?: boolean
  supportsImages?: boolean
  description?: string
}

export type ModelGroupId = 'pinned' | 'recent' | 'preferred' | 'all'

export interface ModelGroup {
  id: ModelGroupId
  label: string
  models: ModelOption[]
}

/**
 * Build ordered dropdown groups for a model list.
 * Models already shown in an earlier group are omitted from later ones so each
 * id appears once. Empty groups are dropped.
 *
 * The "pinned" group matches every variant whose base id is in `prefs.pinned`.
 */
export function groupModelsForSelect(
  models: ModelOption[],
  prefs: ModelPrefs,
): ModelGroup[] {
  const byId = new Map(models.map((m) => [m.id, m]))
  const used = new Set<string>()

  const take = (ids: string[]): ModelOption[] => {
    const out: ModelOption[] = []
    for (const id of ids) {
      if (used.has(id)) continue
      const m = byId.get(id)
      if (!m) continue
      used.add(id)
      out.push(m)
    }
    return out
  }

  const groups: ModelGroup[] = []

  // Pinned: every variant whose base id is pinned. Preserve the order of
  // `models` so variants of the same base stay grouped together, and pinned
  // bases appear in the order they were pinned (first variant encountered).
  const pinnedSet = new Set(prefs.pinned)
  // Walk pinned bases in order; for each, emit all matching variants in
  // `models` order. Models whose base isn't pinned are skipped here.
  const pinnedModels: ModelOption[] = []
  for (const base of prefs.pinned) {
    if (!pinnedSet.has(base)) continue
    for (const m of models) {
      if (used.has(m.id)) continue
      if (parseModelId(m.id).base === base) {
        used.add(m.id)
        pinnedModels.push(m)
      }
    }
  }
  if (pinnedModels.length) groups.push({ id: 'pinned', label: 'Pinned', models: pinnedModels })

  const recent = take(prefs.recent)
  if (recent.length) groups.push({ id: 'recent', label: 'Recent', models: recent })

  const preferredIds = models.filter((m) => m.preferred).map((m) => m.id)
  const preferred = take(preferredIds)
  if (preferred.length) groups.push({ id: 'preferred', label: 'Preferred', models: preferred })

  const rest = models.filter((m) => !used.has(m.id))
  if (rest.length) {
    groups.push({
      id: 'all',
      label: groups.length ? 'All models' : 'Models',
      models: rest,
    })
  } else if (groups.length === 0 && models.length > 0) {
    // Fallback: every model already bucketed into earlier groups that were empty.
    groups.push({ id: 'all', label: 'Models', models })
  }

  return groups
}

/** Prefer: stored selection if still valid, else agent preferred, else first. */
export function pickDefaultModelId(
  models: ModelOption[],
  storedModelId: string,
): string {
  if (storedModelId && models.some((m) => m.id === storedModelId)) {
    return storedModelId
  }
  const preferred = models.find((m) => m.preferred)
  return preferred?.id ?? models[0]?.id ?? ''
}
