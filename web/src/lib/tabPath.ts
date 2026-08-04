/**
 * Joins a workspace root with a relative path using the root's path separator
 * (Windows `\` vs POSIX `/`). Used for "Copy Path" absolute clipboard values.
 */
export function joinWorkspacePath(root: string, relative: string): string {
  const sep = root.includes('\\') ? '\\' : '/'
  const cleaned = relative.replace(/^[/\\]+/, '').replace(/[/\\]+/g, sep)
  const base = root.replace(/[/\\]+$/, '')
  return cleaned ? `${base}${sep}${cleaned}` : base
}

/** True when `path` is `prefix` or a descendant (folder delete / rename remap). */
export function pathIsUnder(path: string, prefix: string): boolean {
  const n = path.replace(/\\/g, '/')
  const p = prefix.replace(/\\/g, '/')
  return n === p || n.startsWith(`${p}/`)
}

/** Remaps a path after a rename of `from` → `to` (exact match or descendant). */
export function remapAfterRename(path: string, from: string, to: string): string {
  const n = path.replace(/\\/g, '/')
  const f = from.replace(/\\/g, '/')
  const t = to.replace(/\\/g, '/')
  if (n === f) return to
  if (n.startsWith(`${f}/`)) return `${t}${n.slice(f.length)}`
  return path
}

/** Browse-preview tab id: `preview:{workspaceId}:{entryPath}`. */
export function previewTabId(workspaceId: string, entryPath: string): string {
  return `preview:${workspaceId}:${entryPath}`
}

/** Joins a folder path with a new child name (POSIX-normalized). */
export function joinUnderParent(parentPath: string, name: string): string {
  const trimmed = name.trim()
  if (!parentPath) return trimmed
  return `${parentPath.replace(/\\/g, '/').replace(/\/$/, '')}/${trimmed}`
}

/**
 * True when an open-tab id is the file at `path`, a descendant file id under
 * a folder `path`, or a browse-preview tab for that path / under it.
 */
export function tabIdTouchesPath(
  tabId: string,
  path: string,
  workspaceId: string | undefined,
): boolean {
  if (tabId === path) return true
  if (!tabId.startsWith('preview:') && pathIsUnder(tabId, path)) return true
  if (!workspaceId || !tabId.startsWith('preview:')) return false
  const prefix = `preview:${workspaceId}:`
  if (!tabId.startsWith(prefix)) return false
  return pathIsUnder(tabId.slice(prefix.length), path)
}

/** Remaps a tab id after renaming `from` → `to` (file tabs and preview tabs). */
export function remapTabIdAfterRename(
  tabId: string,
  from: string,
  to: string,
  workspaceId: string | undefined,
): string {
  if (tabId === from) return to
  if (workspaceId && tabId.startsWith('preview:')) {
    const prefix = `preview:${workspaceId}:`
    if (tabId.startsWith(prefix)) {
      const oldPath = tabId.slice(prefix.length)
      if (pathIsUnder(oldPath, from)) {
        return previewTabId(workspaceId, remapAfterRename(oldPath, from, to))
      }
    }
    return tabId
  }
  if (pathIsUnder(tabId, from)) return remapAfterRename(tabId, from, to)
  return tabId
}
