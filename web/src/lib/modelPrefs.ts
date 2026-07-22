/**
 * Per-agent model preferences stored in localStorage.
 *
 * - favorites: user-pinned model ids (order preserved)
 * - recent: most-recently-used model ids (newest first)
 *
 * Specific model catalogs churn across agents; this module never hardcodes
 * model ids — it only tracks ids the user actually selected.
 */

export interface ModelPrefs {
  favorites: string[]
  recent: string[]
}

const STORAGE_KEY = 'lai:modelPrefs'
const MAX_RECENT = 8
const MAX_FAVORITES = 20

type PrefsByAgent = Record<string, ModelPrefs>

function emptyPrefs(): ModelPrefs {
  return { favorites: [], recent: [] }
}

function readAll(): PrefsByAgent {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as PrefsByAgent
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

function writeAll(all: PrefsByAgent): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all))
  } catch {
    // Quota / private mode — UI still works without persistence.
  }
}

/** Load prefs for one agent harness id. */
export function getModelPrefs(agentId: string): ModelPrefs {
  if (!agentId) return emptyPrefs()
  const entry = readAll()[agentId]
  if (!entry) return emptyPrefs()
  return {
    favorites: Array.isArray(entry.favorites) ? entry.favorites.filter(Boolean) : [],
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

/** Toggle a model in the favorites list. Returns the updated prefs. */
export function toggleFavoriteModel(agentId: string, modelId: string): ModelPrefs {
  const prefs = getModelPrefs(agentId)
  if (!modelId) return prefs
  const isFav = prefs.favorites.includes(modelId)
  const favorites = isFav
    ? prefs.favorites.filter((id) => id !== modelId)
    : [...prefs.favorites, modelId].slice(0, MAX_FAVORITES)
  const next = { ...prefs, favorites }
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

export type ModelGroupId = 'favorites' | 'recent' | 'preferred' | 'all'

export interface ModelGroup {
  id: ModelGroupId
  label: string
  models: ModelOption[]
}

/**
 * Build ordered dropdown groups for a model list.
 * Models already shown in an earlier group are omitted from later ones so each
 * id appears once. Empty groups are dropped.
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

  const fav = take(prefs.favorites)
  if (fav.length) groups.push({ id: 'favorites', label: 'Favorites', models: fav })

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
