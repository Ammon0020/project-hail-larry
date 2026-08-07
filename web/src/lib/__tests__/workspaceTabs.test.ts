import { describe, it, expect } from 'vitest'
import {
  MAX_DRAFT_WORKSPACES,
  persistableTabs,
  planWorkspaceSwitch,
  rememberDrafts,
  resolveWorkspaceTabs,
  toWorkspaceTabs,
} from '../workspaceTabs'
import type { Tab } from '@/types'

function tab(overrides: Partial<Tab> & { id: string }): Tab {
  return {
    name: `${overrides.id}.ts`,
    path: `src/${overrides.id}.ts`,
    content: 'body',
    revision: 1,
    unsaved: false,
    language: 'typescript',
    workspaceId: 'ws-a',
    ...overrides,
  }
}

describe('persistableTabs', () => {
  it('keeps only this workspace and drops synthetic tabs', () => {
    const kept = persistableTabs(
      [
        tab({ id: 'a' }),
        tab({ id: 'b', workspaceId: 'ws-b' }),
        tab({ id: 'settings', kind: 'settings' }),
        tab({ id: 'browse', kind: 'preview' }),
      ],
      'ws-a',
    )

    expect(kept.map((t) => t.id)).toEqual(['a'])
  })

  it('keeps a transient file preview, which is a real file', () => {
    const kept = persistableTabs([tab({ id: 'a', isPreview: true })], 'ws-a')
    expect(kept.map((t) => t.id)).toEqual(['a'])
  })
})

describe('toWorkspaceTabs', () => {
  it('never sends file content to the server', () => {
    const payload = toWorkspaceTabs([tab({ id: 'a' })], 'ws-a', 'a')
    expect(payload.tabs[0]).not.toHaveProperty('content')
    expect(payload.tabs[0]).not.toHaveProperty('unsaved')
  })

  it('drops an active id that is not in the saved set', () => {
    // The active tab may belong to another workspace or be a settings tab.
    const payload = toWorkspaceTabs([tab({ id: 'a' })], 'ws-a', 'settings')
    expect(payload.activeTabId).toBeNull()
  })
})

describe('resolveWorkspaceTabs', () => {
  it('reuses the local buffer so unsaved edits survive a workspace switch', () => {
    const local = tab({ id: 'a', content: 'edited but not saved', unsaved: true })

    const { tabs, needsContent } = resolveWorkspaceTabs(
      { tabs: [{ id: 'a', path: 'src/a.ts', name: 'a.ts' }], activeTabId: 'a' },
      [local],
      'ws-a',
    )

    expect(tabs[0].content).toBe('edited but not saved')
    expect(tabs[0].unsaved).toBe(true)
    expect(needsContent).toEqual([])
  })

  it('flags descriptors with no local copy so the caller can read them', () => {
    const { tabs, needsContent } = resolveWorkspaceTabs(
      { tabs: [{ id: 'remote', path: 'src/x.ts', name: 'x.ts', language: 'typescript' }] },
      [],
      'ws-a',
    )

    expect(needsContent).toEqual(['remote'])
    expect(tabs[0].content).toBe('')
    // Must not look dirty, or the UI would offer to save an empty buffer over
    // a real file.
    expect(tabs[0].unsaved).toBe(false)
    expect(tabs[0].workspaceId).toBe('ws-a')
  })

  // On first run after upgrade the server has no record while this device
  // already has the user's layout. Adopting the empty record would close their
  // editor with no way back.
  it('keeps local tabs when the server has no record yet', () => {
    const local = [tab({ id: 'a' }), tab({ id: 'other', workspaceId: 'ws-b' })]

    const empty = resolveWorkspaceTabs({ tabs: [] }, local, 'ws-a')
    expect(empty.tabs.map((t) => t.id)).toEqual(['a'])
    expect(empty.activeTabId).toBe('a')

    // Same for a failed request.
    const failed = resolveWorkspaceTabs(null, local, 'ws-a')
    expect(failed.tabs.map((t) => t.id)).toEqual(['a'])
  })

  /**
   * The regression this cache exists for.
   *
   * Switching workspaces replaces the whole open-tab set, so the outgoing
   * workspace's buffers leave React state. Without a draft cache the switch
   * back re-reads from disk and the unsaved edit is gone — silently, which is
   * the worst way to lose typing.
   */
  it('restores an unsaved edit after switching away and back', () => {
    const draft = tab({ id: 'a', content: 'typed but never saved', unsaved: true })

    // Leaving ws-a: its buffers are stashed.
    const cache = rememberDrafts({}, 'ws-a', [draft])

    // Now in ws-b. This is what the hook actually holds — ws-a's tabs are gone.
    const openTabsInB = [tab({ id: 'b', workspaceId: 'ws-b' })]

    const { tabs, needsContent } = resolveWorkspaceTabs(
      { tabs: [{ id: 'a', path: 'src/a.ts', name: 'a.ts' }], activeTabId: 'a' },
      openTabsInB,
      'ws-a',
      cache['ws-a'],
    )

    expect(tabs[0].content).toBe('typed but never saved')
    expect(tabs[0].unsaved).toBe(true)
    expect(needsContent).toEqual([])
  })

  it('falls back to the first tab when the saved active id is gone', () => {
    const { activeTabId } = resolveWorkspaceTabs(
      { tabs: [{ id: 'a', path: 'src/a.ts', name: 'a.ts' }], activeTabId: 'deleted' },
      [tab({ id: 'a' })],
      'ws-a',
    )
    expect(activeTabId).toBe('a')
  })
})

describe('rememberDrafts', () => {
  it('stashes only unsaved tabs, since clean ones are re-read from disk', () => {
    const cache = rememberDrafts({}, 'ws-a', [
      tab({ id: 'clean' }),
      tab({ id: 'dirty', unsaved: true }),
      tab({ id: 'elsewhere', unsaved: true, workspaceId: 'ws-b' }),
    ])

    expect(cache['ws-a'].map((t) => t.id)).toEqual(['dirty'])
  })

  /** A stale draft would resurrect old text over a file the user has saved. */
  it('drops the entry once nothing is unsaved', () => {
    const withDraft = rememberDrafts({}, 'ws-a', [tab({ id: 'a', unsaved: true })])
    const afterSave = rememberDrafts(withDraft, 'ws-a', [tab({ id: 'a', unsaved: false })])

    expect(afterSave).not.toHaveProperty('ws-a')
  })

  it('leaves other workspaces untouched', () => {
    const cache = rememberDrafts(
      rememberDrafts({}, 'ws-a', [tab({ id: 'a', unsaved: true })]),
      'ws-b',
      [tab({ id: 'b', unsaved: true, workspaceId: 'ws-b' })],
    )

    expect(Object.keys(cache).sort()).toEqual(['ws-a', 'ws-b'])
  })

  /** Drafts carry file content, so an unbounded cache would fill localStorage. */
  it('caps how many workspaces it retains, evicting the oldest', () => {
    let cache = {}
    for (let i = 0; i <= MAX_DRAFT_WORKSPACES; i++) {
      cache = rememberDrafts(cache, `ws-${i}`, [
        tab({ id: `t${i}`, unsaved: true, workspaceId: `ws-${i}` }),
      ])
    }

    const keys = Object.keys(cache)
    expect(keys).toHaveLength(MAX_DRAFT_WORKSPACES)
    expect(keys).not.toContain('ws-0')
    expect(keys).toContain(`ws-${MAX_DRAFT_WORKSPACES}`)
  })
})

describe('planWorkspaceSwitch', () => {
  /** Same workspace id on both sides means the effect bailed out before calling
   *  this, but the function itself must still be a no-op-ish pass-through: it
   *  stashes the "outgoing" workspace (itself) and resolves against the server
   *  response. The important invariant is that it does not double-stash or
   *  drop drafts it should keep. */
  it('stashes the outgoing workspace and clears the incoming workspace from the cache', () => {
    const outgoing = tab({ id: 'a', content: 'typed', unsaved: true })
    // The incoming workspace already has a stashed draft from a prior visit.
    const incomingDraft = tab({
      id: 'b',
      content: 'old buffer from last visit',
      unsaved: true,
      workspaceId: 'ws-b',
    })
    const cache = rememberDrafts({}, 'ws-b', [incomingDraft])

    const { draftCache } = planWorkspaceSwitch(
      'ws-a',
      [outgoing],
      'a',
      cache,
      { tabs: [{ id: 'b', path: 'src/b.ts', name: 'b.ts' }], activeTabId: 'b' },
      'ws-b',
    )

    // Outgoing ws-a is now stashed; incoming ws-b's draft was consumed.
    expect(draftCache['ws-a'].map((t) => t.id)).toEqual(['a'])
    expect(draftCache).not.toHaveProperty('ws-b')
  })

  /** The whole point of the draft cache: switching back restores the unsaved
   *  buffer instead of re-reading the file from disk. */
  it('restores a cached draft when switching back to that workspace', () => {
    const draft = tab({ id: 'a', content: 'typed but never saved', unsaved: true })
    const cache = rememberDrafts({}, 'ws-a', [draft])

    // Currently in ws-b with no tabs of its own.
    const { tabs, needsContent } = planWorkspaceSwitch(
      'ws-b',
      [tab({ id: 'b', workspaceId: 'ws-b' })],
      'b',
      cache,
      { tabs: [{ id: 'a', path: 'src/a.ts', name: 'a.ts' }], activeTabId: 'a' },
      'ws-a',
    )

    expect(tabs.map((t) => t.id)).toEqual(['a'])
    expect(tabs[0].content).toBe('typed but never saved')
    expect(tabs[0].unsaved).toBe(true)
    expect(needsContent).toEqual([])
  })

  /** A tab the server knows about but this device has never opened has no
   *  buffer — the caller must read it from disk. */
  it('flags server-only tabs as needing content fetch', () => {
    const { tabs, needsContent } = planWorkspaceSwitch(
      'ws-a',
      [tab({ id: 'a' })],
      'a',
      {},
      { tabs: [{ id: 'remote', path: 'src/x.ts', name: 'x.ts', language: 'typescript' }] },
      'ws-b',
    )

    expect(needsContent).toEqual(['remote'])
    expect(tabs.find((t) => t.id === 'remote')?.content).toBe('')
    expect(tabs.find((t) => t.id === 'remote')?.workspaceId).toBe('ws-b')
  })

  /** Settings is synthetic and belongs to no workspace, so a switch must not
   *  close it. If it was active, it stays active. */
  it('carries the settings tab across the switch and keeps it active', () => {
    const settings = tab({ id: 'settings', kind: 'settings' })

    const { tabs, activeTabId } = planWorkspaceSwitch(
      'ws-a',
      [settings, tab({ id: 'a' })],
      'settings',
      {},
      { tabs: [{ id: 'b', path: 'src/b.ts', name: 'b.ts' }], activeTabId: 'b' },
      'ws-b',
    )

    expect(tabs.map((t) => t.id)).toEqual(['b', 'settings'])
    expect(activeTabId).toBe('settings')
  })

  /** When settings is open but not active, the resolved active id wins. */
  it('uses the resolved active id when settings is open but not active', () => {
    const settings = tab({ id: 'settings', kind: 'settings' })

    const { activeTabId } = planWorkspaceSwitch(
      'ws-a',
      [settings, tab({ id: 'a' })],
      'a',
      {},
      { tabs: [{ id: 'b', path: 'src/b.ts', name: 'b.ts' }], activeTabId: 'b' },
      'ws-b',
    )

    expect(activeTabId).toBe('b')
  })

  /** First run / failed fetch: the server has nothing, so the local layout is
   *  adopted rather than silently closing the editor. */
  it('keeps local tabs when the server response is null', () => {
    const local = tab({ id: 'a' })

    const { tabs, activeTabId, needsContent } = planWorkspaceSwitch(
      null,
      [local],
      'a',
      {},
      null,
      'ws-a',
    )

    expect(tabs.map((t) => t.id)).toEqual(['a'])
    expect(activeTabId).toBe('a')
    expect(needsContent).toEqual([])
  })

  /** A failed fetch on a switch *away from* a workspace with unsaved edits must
   *  still stash those edits — the cache is the only thing preserving them. */
  it('stashes outgoing unsaved edits even when the server response is null', () => {
    const outgoing = tab({ id: 'a', content: 'typed', unsaved: true })

    const { draftCache } = planWorkspaceSwitch(
      'ws-a',
      [outgoing],
      'a',
      {},
      null,
      'ws-b',
    )

    expect(draftCache['ws-a'].map((t) => t.id)).toEqual(['a'])
  })

  /** First run has no previous workspace; the stash step is skipped. */
  it('skips the outgoing stash on the first switch (no previous workspace)', () => {
    const { draftCache } = planWorkspaceSwitch(
      null,
      [tab({ id: 'a', unsaved: true })],
      'a',
      {},
      { tabs: [{ id: 'a', path: 'src/a.ts', name: 'a.ts' }], activeTabId: 'a' },
      'ws-a',
    )

    expect(draftCache).toEqual({})
  })
})
