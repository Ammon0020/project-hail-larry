import { useCallback, useEffect, useRef, type MutableRefObject } from 'react'
import { api, type FileNode, type WorkspaceInfo } from '@/lib/api'

/** Return type of api.readFile — file content + metadata. */
interface FileReadResult {
  content: string
  revision: number
  path: string
  isBinary?: boolean
  previewable?: boolean
}

/** Return type of api.saveFile — new revision + path. */
interface FileSaveResult {
  revision: number
  path: string
}

/** Options passed into {@link useFileActions}. */
interface UseFileActionsOptions {
  /** Ref to the active workspace so file actions read the current id without
   *  re-creating callbacks on every workspace switch. */
  activeWorkspaceRef: MutableRefObject<WorkspaceInfo | null>
  /** Setter for the file tree state. */
  setFileTree: React.Dispatch<React.SetStateAction<FileNode[]>>
}

/** Result returned by {@link useFileActions}. */
interface UseFileActionsResult {
  /** Reloads the file tree for the active workspace from the backend. */
  refreshFileTree: () => Promise<void>
  /** Reads a file's content + metadata from the backend. */
  readFile: (path: string, workspaceId?: string) => Promise<FileReadResult>
  /** Saves a file with revision tracking, returning the new revision + path. */
  saveFile: (
    path: string,
    content: string,
    expectedRevision: number,
    workspaceId?: string,
  ) => Promise<FileSaveResult>
  /** Deletes a file (or empty folder) then refreshes the explorer tree. */
  deleteFile: (path: string) => Promise<void>
  /** Renames/moves a path then refreshes the explorer tree. */
  renameFile: (from: string, to: string) => Promise<void>
  /** Creates a directory then refreshes the explorer tree. */
  mkdir: (path: string) => Promise<void>
  /** Creates an empty file (expectedRevision 0) then refreshes the tree. */
  createFile: (path: string) => Promise<void>
  /** Ref holding the latest refreshFileTree — the WS onmessage handler reads
   *  this so reconnects always call the current implementation. The caller
   *  must NOT clear this ref on unmount; it's a stable pointer. */
  refreshFileTreeRef: MutableRefObject<() => Promise<void>>
}

/**
 * Owns file-tree REST actions extracted from `useBackend`.
 *
 * All file actions resolve the workspace id from `activeWorkspaceRef` so they
 * always operate on the current workspace without needing it as a dependency
 * (avoiding callback churn on workspace switches). Mutating actions
 * (delete/rename/mkdir/createFile) refresh the tree afterward so the explorer
 * reflects the change immediately.
 *
 * @param opts Inputs from the host hook.
 * @returns File action callbacks + the refresh ref used by the WS handler.
 */
export function useFileActions({
  activeWorkspaceRef,
  setFileTree,
}: UseFileActionsOptions): UseFileActionsResult {
  // Latest refreshFileTree — connectWebSocket's onmessage closes over this ref
  // so reconnects always call the current implementation.
  const refreshFileTreeRef = useRef<() => Promise<void>>(async () => {})

  /**
   * Reloads the file tree for the active workspace from the backend. Called
   * after a FileWritten / FileChangedOnDisk event (debounced) so the explorer
   * reflects agent-created and external files without a manual refresh.
   */
  const refreshFileTree = useCallback(async () => {
    const ws = activeWorkspaceRef.current
    if (!ws) return
    try {
      setFileTree(await api.getFileTree(ws.id))
    } catch {
      // Workspace may have been removed; leave the tree as-is.
    }
  }, [activeWorkspaceRef, setFileTree])

  useEffect(() => {
    refreshFileTreeRef.current = refreshFileTree
  }, [refreshFileTree])

  const readFile = useCallback(async (path: string, workspaceId?: string) => {
    const wsId = workspaceId || activeWorkspaceRef.current?.id || ''
    return await api.readFile(wsId, path)
  }, [activeWorkspaceRef])

  const saveFile = useCallback(
    async (path: string, content: string, expectedRevision: number, workspaceId?: string) => {
      const wsId = workspaceId || activeWorkspaceRef.current?.id || ''
      return await api.saveFile(wsId, path, content, expectedRevision)
    },
    [activeWorkspaceRef],
  )

  /** Deletes a file (or empty folder) then refreshes the explorer tree. */
  const deleteFile = useCallback(
    async (path: string) => {
      const wsId = activeWorkspaceRef.current?.id || ''
      await api.deleteFile(wsId, path)
      await refreshFileTree()
    },
    [activeWorkspaceRef, refreshFileTree],
  )

  /** Renames/moves a path then refreshes the explorer tree. */
  const renameFile = useCallback(
    async (from: string, to: string) => {
      const wsId = activeWorkspaceRef.current?.id || ''
      await api.renameFile(wsId, from, to)
      await refreshFileTree()
    },
    [activeWorkspaceRef, refreshFileTree],
  )

  /** Creates a directory then refreshes the explorer tree. */
  const mkdir = useCallback(
    async (path: string) => {
      const wsId = activeWorkspaceRef.current?.id || ''
      await api.mkdir(wsId, path)
      await refreshFileTree()
    },
    [activeWorkspaceRef, refreshFileTree],
  )

  /** Creates an empty file (expectedRevision 0) then refreshes the tree. */
  const createFile = useCallback(
    async (path: string) => {
      const wsId = activeWorkspaceRef.current?.id || ''
      await api.saveFile(wsId, path, '', 0)
      await refreshFileTree()
    },
    [activeWorkspaceRef, refreshFileTree],
  )

  return {
    refreshFileTree,
    readFile,
    saveFile,
    deleteFile,
    renameFile,
    mkdir,
    createFile,
    refreshFileTreeRef,
  }
}
