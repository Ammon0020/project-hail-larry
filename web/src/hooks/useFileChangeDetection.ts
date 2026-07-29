import { useEffect, useRef, type Dispatch, type SetStateAction } from 'react'
import type { useBackend } from '@/hooks/useBackend'
import type { Tab } from '@/types'

type FileChangeBackend = Pick<
  ReturnType<typeof useBackend>,
  'activeWorkspace' | 'events' | 'readFile'
>

/**
 * Reconciles open editor tabs with file writes reported by the backend.
 * Clean tabs refresh silently, while dirty tabs retain their content and are
 * marked as changed on disk so the user can choose whether to reload them.
 */
export function useFileChangeDetection(
  backend: FileChangeBackend,
  openTabs: Tab[],
  setOpenTabs: Dispatch<SetStateAction<Tab[]>>,
): void {
  const processedFileEventIdRef = useRef<number | null>(null)
  // Pull the backend fields used as effect dependencies out of the backend
  // object so the react-hooks/exhaustive-deps rule sees standalone identifiers
  // (the backend object is new each render, but events/activeWorkspace/readFile
  // are stable enough as deps given the processedFileEventIdRef guard).
  const { activeWorkspace, events, readFile } = backend

  useEffect(() => {
    if (events.length === 0) return
    const maxId = Math.max(...events.map((event) => event.id ?? 0))
    if (processedFileEventIdRef.current === null) {
      processedFileEventIdRef.current = maxId
      return
    }
    const since = processedFileEventIdRef.current
    if (maxId <= since) return
    processedFileEventIdRef.current = maxId
    const changedPaths = new Set(
      events
        .filter(
          (event) =>
            (event.id ?? 0) > since &&
            (event.type === 'FileWritten' || event.type === 'FileChangedOnDisk') &&
            !!event.target &&
            (!event.workspaceId ||
              !activeWorkspace ||
              event.workspaceId === activeWorkspace.id),
        )
        .map((event) => event.target as string),
    )
    if (changedPaths.size === 0) return

    setOpenTabs((currentTabs) =>
      currentTabs.map((tab) =>
        changedPaths.has(tab.path) && tab.unsaved ? { ...tab, changedOnDisk: true } : tab,
      ),
    )

    for (const tab of openTabs) {
      if (tab.kind === 'settings' || tab.kind === 'preview') continue
      if (!changedPaths.has(tab.path) || tab.unsaved) continue
      readFile(tab.path, tab.workspaceId)
        .then((file) => {
          setOpenTabs((currentTabs) =>
            currentTabs.map((currentTab) =>
              currentTab.id === tab.id &&
              !currentTab.unsaved &&
              file.revision > currentTab.revision
                ? {
                    ...currentTab,
                    content: file.content,
                    revision: file.revision,
                    isBinary: file.isBinary ?? false,
                    previewable: file.previewable ?? false,
                    changedOnDisk: false,
                  }
                : currentTab,
            ),
          )
        })
        .catch(() => {})
    }
  }, [events, activeWorkspace, readFile, openTabs, setOpenTabs])
}
