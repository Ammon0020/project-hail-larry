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

  useEffect(() => {
    const events = backend.events
    if (events.length === 0) return
    const maxId = Math.max(...events.map((event) => event.id ?? 0))
    if (processedFileEventIdRef.current === null) {
      processedFileEventIdRef.current = maxId
      return
    }
    const since = processedFileEventIdRef.current
    if (maxId <= since) return
    processedFileEventIdRef.current = maxId
    const activeWorkspace = backend.activeWorkspace
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
      if (!changedPaths.has(tab.path) || tab.unsaved) continue
      backend
        .readFile(tab.path, tab.workspaceId)
        .then((file) => {
          setOpenTabs((currentTabs) =>
            currentTabs.map((currentTab) =>
              currentTab.id === tab.id && !currentTab.unsaved
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
    // useBackend actions are not stable yet; backend.events is the event cursor.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [backend.events])
}
