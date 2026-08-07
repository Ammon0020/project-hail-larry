import type { WorkspaceTab, WorkspaceTabs } from '@/lib/api'
import { safeStorage } from '@/lib/safeStorage'
import type { Tab } from '@/types'

/**
 * Tab kinds that are synthetic and must never be persisted or restored.
 *
 * `settings` belongs to no workspace, and `preview` tabs are throwaway iframes.
 * A transient *file* preview (`isPreview`) is different — it is a real file and
 * is kept, flag intact, so the preview slot survives a reload.
 */
const EPHEMERAL_KINDS = new Set(['settings', 'preview'])

/**
 * Unsaved buffers for workspaces the user is not currently looking at.
 *
 * Switching workspaces swaps the entire open-tab set, so the outgoing
 * workspace's buffers leave React state — and `lai:openTabs` is rewritten with
 * the incoming set. Without this stash, switching away and back would re-read
 * every file from disk and discard whatever the user had typed.
 *
 * Only *unsaved* tabs are kept. A clean tab's content is on disk already, so
 * re-reading it is both cheaper and fresher than trusting a stale copy.
 */
export type TabDraftCache = Record<string, Tab[]>

const DRAFTS_KEY = 'lai:tabDrafts'

/**
 * How many workspaces' drafts to retain. Drafts carry full file content, so an
 * unbounded cache would grow until localStorage rejects the write.
 */
export const MAX_DRAFT_WORKSPACES = 8

/**
 * Records `workspaceId`'s unsaved buffers, returning a new cache.
 *
 * Called when leaving a workspace. Recomputing on every departure is what
 * keeps the stash honest: a tab the user saved is no longer unsaved, so its
 * draft disappears rather than lingering to overwrite the saved file later.
 */
export function rememberDrafts(
  cache: TabDraftCache,
  workspaceId: string,
  tabs: Tab[],
): TabDraftCache {
  const drafts = persistableTabs(tabs, workspaceId).filter((tab) => tab.unsaved)
  const next = { ...cache }
  delete next[workspaceId]
  if (drafts.length === 0) return next

  // Re-inserting moves the key to the end, so eviction takes the workspace
  // whose drafts have gone longest without being touched.
  next[workspaceId] = drafts
  const keys = Object.keys(next)
  for (const stale of keys.slice(0, Math.max(0, keys.length - MAX_DRAFT_WORKSPACES))) {
    delete next[stale]
  }
  return next
}

/** Reads the stash. Corrupt or unavailable storage degrades to "no drafts". */
export function loadTabDrafts(): TabDraftCache {
  return safeStorage.getJson<TabDraftCache>(DRAFTS_KEY, {})
}

/** Persists the stash so drafts survive a reload, not just a switch. */
export function saveTabDrafts(cache: TabDraftCache): void {
  if (Object.keys(cache).length === 0) safeStorage.remove(DRAFTS_KEY)
  else safeStorage.setJson(DRAFTS_KEY, cache)
}

/** Tabs that belong to `workspaceId` and are worth remembering. */
export function persistableTabs(tabs: Tab[], workspaceId: string): Tab[] {
  return tabs.filter(
    (tab) => tab.workspaceId === workspaceId && !EPHEMERAL_KINDS.has(tab.kind ?? 'file'),
  )
}

/** Strip a tab down to what the server stores — identity and order, no content. */
export function toWorkspaceTab(tab: Tab): WorkspaceTab {
  return {
    id: tab.id,
    path: tab.path,
    name: tab.name,
    language: tab.language,
    kind: tab.kind,
    isPreview: tab.isPreview,
    viewMode: tab.viewMode,
    staged: tab.staged,
    commitOid: tab.commitOid,
  }
}

/** The payload for `PUT /workspaces/:id/tabs`. */
export function toWorkspaceTabs(
  tabs: Tab[],
  workspaceId: string,
  activeTabId: string | null,
): WorkspaceTabs {
  const scoped = persistableTabs(tabs, workspaceId)
  return {
    tabs: scoped.map(toWorkspaceTab),
    // Only claim an active tab that is actually in the saved set.
    activeTabId: scoped.some((tab) => tab.id === activeTabId) ? activeTabId : null,
  }
}

/**
 * Rebuild editor tabs for a workspace from its saved descriptors.
 *
 * Content comes from `cached` when the device already has the tab — that
 * preserves unsaved edits across a workspace switch. Descriptors with no cached
 * content are returned with empty content and `needsContent` set, so the caller
 * can read them from disk; that is the fresh-device and cleared-storage path.
 */
export function resolveWorkspaceTabs(
  saved: WorkspaceTabs | null,
  cached: Tab[],
  workspaceId: string,
  drafts: Tab[] = [],
): { tabs: Tab[]; needsContent: string[]; activeTabId: string | null } {
  // An empty server record is not the same as "this workspace has no tabs".
  // On first run after upgrade — and whenever the request fails — the server
  // has nothing while this device already has the user's layout. Adopting the
  // empty record would silently close their editor; the debounced save then
  // uploads the local set, so the server catches up on its own.
  const available = mergeDrafts(persistableTabs(cached, workspaceId), drafts)
  if (!saved || saved.tabs.length === 0) {
    return { tabs: available, needsContent: [], activeTabId: available[0]?.id ?? null }
  }
  const { tabs, needsContent } = hydrateTabs(saved, available, workspaceId)
  const activeTabId =
    saved.activeTabId && tabs.some((tab) => tab.id === saved.activeTabId)
      ? saved.activeTabId
      : (tabs[0]?.id ?? null)
  return { tabs, needsContent, activeTabId }
}

/** Stashed drafts win over a live tab of the same id: they are the newer text. */
function mergeDrafts(local: Tab[], drafts: Tab[]): Tab[] {
  if (drafts.length === 0) return local
  const byId = new Map(local.map((tab) => [tab.id, tab]))
  for (const draft of drafts) byId.set(draft.id, draft)
  return [...byId.values()]
}

function hydrateTabs(
  saved: WorkspaceTabs,
  cached: Tab[],
  workspaceId: string,
): { tabs: Tab[]; needsContent: string[] } {
  const byId = new Map(cached.map((tab) => [tab.id, tab]))
  const needsContent: string[] = []
  const tabs = saved.tabs.map((descriptor) => {
    const local = byId.get(descriptor.id)
    if (local) {
      // Keep the local buffer verbatim: it may hold unsaved edits.
      return local
    }
    needsContent.push(descriptor.id)
    return {
      id: descriptor.id,
      name: descriptor.name,
      path: descriptor.path,
      content: '',
      revision: 0,
      unsaved: false,
      language: descriptor.language ?? '',
      workspaceId,
      kind: descriptor.kind as Tab['kind'],
      isPreview: descriptor.isPreview,
      viewMode: descriptor.viewMode as Tab['viewMode'],
      staged: descriptor.staged,
      commitOid: descriptor.commitOid,
    } satisfies Tab
  })
  return { tabs, needsContent }
}
