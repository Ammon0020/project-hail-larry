import { useEffect, useState, type Dispatch, type MutableRefObject, type SetStateAction } from 'react'
import { editorTabPreviewState } from '@/components/tabPreviewState'
import { useBackend } from '@/hooks/useBackend'
import { joinUnderParent, previewTabId } from '@/lib/tabPath'
import type { MobileView, Tab } from '@/types'

type Backend = ReturnType<typeof useBackend>

interface UseEditorTabHandlersOptions {
  openTabs: Tab[]
  setOpenTabs: Dispatch<SetStateAction<Tab[]>>
  activeTabId: string | null
  setActiveTabId: Dispatch<SetStateAction<string | null>>
  fileSelectTokenRef: MutableRefObject<number>
  handleToggleViewMode: (tabId: string) => void
  isDesktop: boolean
  setMobileView: Dispatch<SetStateAction<MobileView>>
  backend: Backend
}

export function useEditorTabHandlers({
  openTabs,
  setOpenTabs,
  activeTabId,
  setActiveTabId,
  fileSelectTokenRef,
  handleToggleViewMode,
  isDesktop,
  setMobileView,
  backend,
}: UseEditorTabHandlersOptions) {
  // Line to scroll to when a search result is clicked. Set by
  // handleSearchResultSelect and consumed by EditorPane's scrollToLine prop.
  // Cleared after the editor processes the jump so the same line number can
  // re-trigger (e.g. clicking a result in a different file at the same line).
  const [searchResultLine, setSearchResultLine] = useState<number | null>(null)

  // Clear the search-result line target after the editor has had a chance to
  // dispatch the jump. Using setTimeout(0) defers the clear to the next
  // macrotask, which runs after the EditorPane effect that performs the
  // scroll. This ensures a subsequent click on the same line number (e.g. in
  // a different file) re-triggers the scrollToLine effect.
  useEffect(() => {
    if (searchResultLine == null) return
    const timer = setTimeout(() => setSearchResultLine(null), 0)
    return () => clearTimeout(timer)
  }, [searchResultLine])

  // ---- File operations ----
  const handleFileSelect = async (path: string): Promise<boolean> => {
    // Check if tab already open (file tabs only — preview tabs share path)
    const existing = openTabs.find((t) => t.path === path && t.kind !== 'preview')
    if (existing) {
      setActiveTabId(existing.id)
      if (!isDesktop) setMobileView('editor')
      return true
    }
    // Capture a token so a newer open request can supersede this one's
    // post-await setState calls (shared preview slot race).
    const token = ++fileSelectTokenRef.current
    // Load file from backend
    try {
      const file = await backend.readFile(path)
      if (token !== fileSelectTokenRef.current) return false
      const name = path.split(/[\\/]/).pop() || path
      const ext = name.split('.').pop() || ''
      const tab: Tab = {
        id: path,
        name,
        path,
        content: file.content,
        revision: file.revision,
        unsaved: false,
        language: ext.toLowerCase(),
        isBinary: file.isBinary ?? false,
        previewable: file.previewable ?? false,
        workspaceId: backend.activeWorkspace?.id,
        isPreview: true,
      }
      setOpenTabs((prev) => {
        const previewIdx = prev.findIndex((t) => t.isPreview)
        if (previewIdx === -1) return [...prev, tab]
        const next = prev.slice()
        next[previewIdx] = tab
        return next
      })
      setActiveTabId(path)
      if (!isDesktop) setMobileView('editor')
      return true
    } catch (err) {
      if (token !== fileSelectTokenRef.current) return false
      console.error('Failed to open file:', err)
      window.alert(err instanceof Error ? `Failed to open file: ${err.message}` : 'Failed to open file')
      return false
    }
  }

  /** Opens a persistent browse-preview tab for an HTML entry point. */
  const handleOpenPreview = (entryPath: string) => {
    const workspaceId = backend.activeWorkspace?.id
    if (!workspaceId) return
    const tabId = previewTabId(workspaceId, entryPath)
    const existing = openTabs.find((t) => t.id === tabId)
    if (existing) {
      setActiveTabId(existing.id)
      if (!isDesktop) setMobileView('editor')
      return
    }
    const name = entryPath.split(/[\\/]/).pop() || entryPath
    const tab: Tab = {
      id: tabId,
      name: `Preview: ${name}`,
      path: entryPath,
      content: '',
      revision: 0,
      unsaved: false,
      language: 'html',
      kind: 'preview',
      workspaceId,
      isPreview: false,
    }
    setOpenTabs((prev) => [...prev, tab])
    setActiveTabId(tabId)
    if (!isDesktop) setMobileView('editor')
  }

  /** Opens a persistent tab for the selected index or worktree version of a changed file. */
  const handleOpenDiff = (path: string, staged: boolean) => {
    const workspaceId = backend.activeWorkspace?.id
    if (!workspaceId) return
    const tabId = `git-diff:${staged ? 'staged' : 'worktree'}:${path}`
    const existing = openTabs.find((tab) => tab.id === tabId)
    if (existing) {
      setActiveTabId(existing.id)
    } else {
      const name = path.split(/[\\/]/).pop() || path
      setOpenTabs((prev) => [...prev, {
        id: tabId,
        name: `Diff: ${name}`,
        path,
        content: '',
        revision: 0,
        unsaved: false,
        language: '',
        kind: 'git-diff',
        workspaceId,
        staged,
        isPreview: false,
      }])
      setActiveTabId(tabId)
    }
    if (!isDesktop) setMobileView('editor')
  }

  /** Opens a persistent tab for all files changed by one history commit. */
  const handleOpenCommitDiff = (commitOid: string) => {
    const workspaceId = backend.activeWorkspace?.id
    if (!workspaceId) return
    const tabId = `git-commit-diff:${commitOid}`
    const existing = openTabs.find((tab) => tab.id === tabId)
    if (existing) {
      setActiveTabId(existing.id)
    } else {
      setOpenTabs((prev) => [...prev, {
        id: tabId,
        name: `Commit: ${commitOid.slice(0, 8)}`,
        path: commitOid,
        content: '',
        revision: 0,
        unsaved: false,
        language: '',
        kind: 'git-commit-diff',
        workspaceId,
        commitOid,
        isPreview: false,
      }])
      setActiveTabId(tabId)
    }
    if (!isDesktop) setMobileView('editor')
  }

  /** Prompts for a name and creates an empty file under the folder, then opens it. */
  const handleTreeNewFile = async (parentPath: string) => {
    const name = window.prompt('New file name')
    if (!name?.trim()) return
    const rel = joinUnderParent(parentPath, name)
    try {
      await backend.createFile(rel)
      await handleFileSelect(rel)
    } catch (err) {
      console.error('Create file failed:', err)
      window.alert(err instanceof Error ? err.message : 'Create file failed')
    }
  }

  /** Prompts for a name and creates a folder under the parent path. */
  const handleTreeNewFolder = async (parentPath: string) => {
    const name = window.prompt('New folder name')
    if (!name?.trim()) return
    const rel = joinUnderParent(parentPath, name)
    try {
      await backend.mkdir(rel)
    } catch (err) {
      console.error('Create folder failed:', err)
      window.alert(err instanceof Error ? err.message : 'Create folder failed')
    }
  }

  // Desktop header TabBar Preview button (EditorPane uses the same helper).
  const activeEditorTab = openTabs.find((t) => t.id === activeTabId) ?? null
  const previewUi = editorTabPreviewState(activeEditorTab)
  const handleTabBarPreview = () => {
    if (!activeEditorTab) return
    if (previewUi.isHtmlEntry) {
      handleOpenPreview(activeEditorTab.path)
      return
    }
    handleToggleViewMode(activeEditorTab.id)
  }

  // Opens a file from a search result and jumps the editor cursor to the
  // matched line. If the file is already open in a tab, just activates it and
  // sets the line; otherwise loads the file first, then sets the line after
  // the content is available so the editor can resolve the line position.
  const handleSearchResultSelect = async (path: string, lineNumber: number): Promise<void> => {
    const existing = openTabs.find((t) => t.path === path && t.kind !== 'preview')
    if (existing) {
      setActiveTabId(existing.id)
    } else {
      const token = fileSelectTokenRef.current
      const ok = await handleFileSelect(path)
      if (!ok || token !== fileSelectTokenRef.current) return
    }
    setSearchResultLine(lineNumber)
  }

  return {
    searchResultLine,
    previewUi,
    handleFileSelect,
    handleOpenPreview,
    handleOpenDiff,
    handleOpenCommitDiff,
    handleTreeNewFile,
    handleTreeNewFolder,
    handleTabBarPreview,
    handleSearchResultSelect,
  }
}
