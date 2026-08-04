import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from 'react'
import { type SettingsSection } from '@/components/SettingsPanel'
import { useBackend } from '@/hooks/useBackend'
import { useFileChangeDetection } from '@/hooks/useFileChangeDetection'
import { pathIsUnder, previewTabId, remapAfterRename, remapTabIdAfterRename, tabIdTouchesPath } from '@/lib/tabPath'
import type { EditorSelectionInfo } from '@/lib/api'
import type { Tab } from '@/types'

type Backend = ReturnType<typeof useBackend>

interface UseTabManagerOptions {
  setSettingsSection: Dispatch<SetStateAction<SettingsSection>>
  backend: Backend
  saveFile: Backend['saveFile']
  readFile: Backend['readFile']
  reportContext: Backend['reportContext']
  editorSelection: EditorSelectionInfo | undefined
  activeSessionId: string | null
}

export function useTabManager({
  setSettingsSection,
  backend,
  saveFile,
  readFile,
  reportContext,
  editorSelection,
  activeSessionId,
}: UseTabManagerOptions) {
  // Tab state — restored from localStorage so open files survive a reload.
  const [openTabs, setOpenTabs] = useState<Tab[]>(() => {
    try {
      const stored = localStorage.getItem('lai:openTabs')
      return stored ? (JSON.parse(stored) as Tab[]) : []
    } catch {
      return []
    }
  })
  const [activeTabId, setActiveTabId] = useState<string | null>(
    () => localStorage.getItem('lai:activeTabId') || null,
  )
  const lastCodeTabIdRef = useRef<string | null>(
    activeTabId !== 'settings' ? activeTabId : null
  )

  // Save error — shown as a transient banner so save failures aren't silent
  // (previously only console.error'd, making debugging impossible).
  const [saveError, setSaveError] = useState<string | null>(null)

  // Monotonic token for handleFileSelect: a stale async resolution bails out
  // of setState if a newer open request has superseded it (preview-slot race).
  const fileSelectTokenRef = useRef(0)

  useEffect(() => {
    if (activeTabId && activeTabId !== 'settings') {
      lastCodeTabIdRef.current = activeTabId
    }
  }, [activeTabId])

  /** Persist open tabs, active tab, panel, and mobile view so the layout
   *  survives a page reload (UI Spec §6.2 — UI Persistence). */
  useEffect(() => {
    try {
      // Settings and browse-preview tabs are synthetic / session-only — not
      // persisted. Transient isPreview file tabs ARE persisted (with their
      // isPreview flag intact) so a reload restores the editor's tab state
      // including the single preview slot; "Keep Open" is about promoting a
      // preview tab to persistent, not about reload survival.
      const persistable = openTabs.filter(
        (t) => t.kind !== 'settings' && t.kind !== 'preview',
      )
      localStorage.setItem('lai:openTabs', JSON.stringify(persistable))
    } catch {
      // Ignore serialization errors (e.g. quota exceeded).
    }
  }, [openTabs])

  useEffect(() => {
    if (activeTabId) localStorage.setItem('lai:activeTabId', activeTabId)
    else localStorage.removeItem('lai:activeTabId')
  }, [activeTabId])

  // Report open files and recent (unsaved) edits to the backend so the context
  // middleware can inject them into the next agent prompt. Debounced inside
  // backend.reportContext (~1s) so rapid tab switches don't flood the API.
  // Skipped when there's no active session or no active workspace. The current
  // editor selection is included so the backend can emit it as a resource block
  // (ACP spec item 1.3).
  useEffect(() => {
    if (!activeSessionId || !backend.activeWorkspace) return
    const openFiles = openTabs
      .filter((t) => t.kind !== 'settings' && t.kind !== 'preview')
      .map((t) => t.path)
    const recentEdits = openTabs.filter((t) => t.unsaved).map((t) => t.path)
    reportContext(activeSessionId, openFiles, recentEdits, editorSelection)
  }, [openTabs, activeSessionId, editorSelection, backend.activeWorkspace, reportContext])

  useFileChangeDetection(backend, openTabs, setOpenTabs)

  // ---- Tab operations ----
  // Defined before the unpaired early return so the keyboard-shortcut
  // useEffect below them is not called conditionally (react-hooks/rules-of-hooks).
  const handleTabSelect = (id: string) => {
    setActiveTabId(id)
    setOpenTabs((prev) =>
      prev.map((t) => (t.id === id && t.isPreview ? { ...t, isPreview: false } : t)),
    )
  }

  /** Opens the settings tab (singleton id 'settings'). If already open,
   *  activates it; otherwise creates and activates it. Optional `section`
   *  focuses Agents / MCP / General. Settings tabs are not persisted to
   *  localStorage (filtered out in the persistence effect). */
  const openSettingsTab = useCallback((section?: SettingsSection) => {
    if (section) setSettingsSection(section)
    setOpenTabs((prev) => {
      if (prev.some((t) => t.id === 'settings')) return prev
      return [...prev, {
        id: 'settings',
        name: 'Settings',
        path: 'settings',
        content: '',
        revision: 0,
        unsaved: false,
        language: '',
        kind: 'settings' as const,
      }]
    })
    setActiveTabId('settings')
  }, [setSettingsSection])

  const handleTabClose = useCallback(
    (id: string) => {
      const tab = openTabs.find((t) => t.id === id)
      if (tab?.unsaved && !window.confirm(`Close "${tab.name}" without saving? Unsaved edits will be lost.`)) return
      setOpenTabs((prev) => {
        const idx = prev.findIndex((t) => t.id === id)
        const next = prev.filter((t) => t.id !== id)
        if (activeTabId === id && next.length > 0) {
          setActiveTabId(next[Math.min(Math.max(0, idx - 1), next.length - 1)].id)
        }
        return next
      })
    },
    [activeTabId, openTabs],
  )

  /** Close every tab except the given one (settings tabs are always kept). */
  const handleCloseOthers = useCallback(
    (id: string) => {
      const dropping = openTabs.filter((t) => t.id !== id && t.kind !== 'settings' && t.unsaved)
      if (dropping.length > 0 && !window.confirm(`Close ${dropping.length} unsaved tab(s) without saving? Edits will be lost.`)) return
      setOpenTabs((prev) => {
        const next = prev.filter((t) => t.id === id || t.kind === 'settings')
        if (activeTabId && !next.some((t) => t.id === activeTabId)) {
          setActiveTabId(next.length > 0 ? next[next.length - 1].id : null)
        }
        return next
      })
    },
    [activeTabId, openTabs],
  )

  /** Close all saved (non-unsaved) tabs except the given one. Settings tabs
   *  are always kept. */
  const handleCloseSaved = useCallback(
    (id: string) => {
      setOpenTabs((prev) => {
        const next = prev.filter((t) => t.unsaved || t.id === id || t.kind === 'settings')
        if (activeTabId && !next.some((t) => t.id === activeTabId)) {
          setActiveTabId(next.length > 0 ? next[next.length - 1].id : null)
        }
        return next
      })
    },
    [activeTabId],
  )

  /** Close all tabs to the right of the given tab (settings tabs are kept). */
  const handleCloseToRight = useCallback(
    (id: string) => {
      const idx = openTabs.findIndex((t) => t.id === id)
      if (idx === -1) return
      const dropping = openTabs.filter((t, i) => i > idx && t.kind !== 'settings' && t.unsaved)
      if (dropping.length > 0 && !window.confirm(`Close ${dropping.length} unsaved tab(s) without saving? Edits will be lost.`)) return
      setOpenTabs((prev) => {
        const next = prev.filter((t, i) => i <= idx || t.kind === 'settings')
        if (activeTabId && !next.some((t) => t.id === activeTabId)) {
          setActiveTabId(next.length > 0 ? next[next.length - 1].id : null)
        }
        return next
      })
    },
    [activeTabId, openTabs],
  )

  /** Renames a file/folder via API, remaps open tabs, refreshes the tree. */
  const handleTreeRename = useCallback(
    async (from: string, to: string) => {
      try {
        await backend.renameFile(from, to)
        const wsId = backend.activeWorkspace?.id
        setOpenTabs((prev) =>
          prev.map((t) => {
            if (t.kind === 'settings') return t
            if (!pathIsUnder(t.path, from)) return t
            const newPath = remapAfterRename(t.path, from, to)
            const name = newPath.split(/[\\/]/).pop() || newPath
            if (t.kind === 'preview') {
              return {
                ...t,
                id: wsId ? previewTabId(wsId, newPath) : t.id,
                path: newPath,
                name: `Preview: ${name}`,
              }
            }
            return { ...t, id: newPath, path: newPath, name }
          }),
        )
        setActiveTabId((prev) =>
          prev ? remapTabIdAfterRename(prev, from, to, wsId) : prev,
        )
      } catch (err) {
        console.error('Rename failed:', err)
        window.alert(err instanceof Error ? err.message : 'Rename failed')
      }
    },
    [backend],
  )

  /** Deletes a file/folder after confirm; closes matching tabs. */
  const handleTreeDelete = useCallback(
    async (path: string, kind: 'file' | 'folder') => {
      const label = kind === 'folder' ? `folder "${path}"` : `"${path}"`
      if (!window.confirm(`Delete ${label}? This cannot be undone.`)) return
      try {
        await backend.deleteFile(path)
        const wsId = backend.activeWorkspace?.id
        setOpenTabs((prev) => {
          const next = prev.filter(
            (t) => t.kind === 'settings' || !pathIsUnder(t.path, path),
          )
          setActiveTabId((active) => {
            if (!active || !tabIdTouchesPath(active, path, wsId)) return active
            return next.length > 0 ? next[next.length - 1].id : null
          })
          return next
        })
      } catch (err) {
        console.error('Delete failed:', err)
        window.alert(err instanceof Error ? err.message : 'Delete failed')
      }
    },
    [backend],
  )

  const handleKeepOpen = useCallback((id: string) => {
    setOpenTabs((prev) => prev.map((t) => (t.id === id ? { ...t, isPreview: false } : t)))
  }, [])

  const handleContentChange = (content: string) => {
    setOpenTabs((prev) =>
      prev.map((t) =>
        t.id === activeTabId ? { ...t, content, unsaved: true, isPreview: false } : t,
      ),
    )
  }

  const handleSave = useCallback(async () => {
    const tab = openTabs.find((t) => t.id === activeTabId)
    if (!tab) return
    try {
      const result = await saveFile(tab.path, tab.content, tab.revision, tab.workspaceId)
      setOpenTabs((prev) =>
        prev.map((t) =>
          t.id === activeTabId
            ? { ...t, revision: result.revision, unsaved: false, changedOnDisk: false }
            : t,
        ),
      )
      setSaveError(null)
    } catch (err) {
      console.error('Save failed:', err)
      setSaveError(err instanceof Error ? err.message : String(err))
    }
  }, [saveFile, openTabs, activeTabId])

  /** Reloads a tab's content from disk, discarding local edits. Invoked from
   *  the EditorPane "changed on disk" banner's Reload action. */
  const handleReloadTab = useCallback(
    async (tabId: string) => {
      const tab = openTabs.find((t) => t.id === tabId)
      if (!tab) return
      try {
        const file = await readFile(tab.path, tab.workspaceId)
        setOpenTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  content: file.content,
                  revision: file.revision,
                  isBinary: file.isBinary ?? false,
                  previewable: file.previewable ?? false,
                  unsaved: false,
                  changedOnDisk: false,
                }
              : t,
          ),
        )
        setSaveError(null)
      } catch (err) {
        setSaveError(err instanceof Error ? err.message : String(err))
      }
    },
    [readFile, openTabs],
  )

  /** Toggles a text-preview tab between edit (CodeMirror) and preview
   *  (FileViewer) modes. Only applies to files with previewable=true and
   *  isBinary=false — binary files always show FileViewer. */
  const handleToggleViewMode = useCallback((tabId: string) => {
    setOpenTabs((prev) =>
      prev.map((t) =>
        t.id === tabId && t.previewable && !t.isBinary
          ? { ...t, viewMode: t.viewMode === 'preview' ? 'edit' : 'preview' }
          : t,
      ),
    )
  }, [])

  return {
    openTabs,
    setOpenTabs,
    activeTabId,
    setActiveTabId,
    lastCodeTabIdRef,
    fileSelectTokenRef,
    saveError,
    setSaveError,
    handleTabSelect,
    openSettingsTab,
    handleTabClose,
    handleCloseOthers,
    handleCloseSaved,
    handleCloseToRight,
    handleTreeRename,
    handleTreeDelete,
    handleKeepOpen,
    handleContentChange,
    handleSave,
    handleReloadTab,
    handleToggleViewMode,
  }
}
