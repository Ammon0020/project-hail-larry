import { useCallback, useMemo, useRef, useState } from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import {
  ChevronRight,
  ChevronDown,
  Folder,
  FolderOpen,
  Circle,
  Eye,
  File,
  FilePlus,
  FolderPlus,
  Pencil,
  Trash2,
  Copy,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { FileIcon } from '@/lib/fileIcon'
import type { FileTreeNode } from '@/types'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

/**
 * Row variants for every tree-node kind. The base carries the shared layout
 * (flex row, padding, rounding, cursor); each variant only adds the colors and
 * state styling that differ. This keeps the JSX readable — call sites say
 * `rowStyles({ kind: 'folder' })` instead of re-stating the full class soup.
 *
 * Base gap is gap-1.5 (6px); the chevron is w-3.5 (14px). Together they're the
 * 20px indent unit used by the children wrapper so nested icons line up under
 * their parent folder's icon.
 */
const rowStyles = cva(
  'flex items-center gap-1.5 p-1 rounded cursor-pointer',
  {
    variants: {
      kind: {
        folder: 'hover:bg-accent text-foreground',
        active: 'bg-primary/10 text-primary border-l-2 border-primary',
        default: 'hover:bg-accent text-muted-foreground',
      },
      // Orthogonal to `kind`: a blue outline marks the row whose context menu
      // is currently open (the right-clicked item). Uses `ring-primary` so it
      // adapts to the active theme via the `--ring`/`--primary` token.
      menuOpen: {
        true: 'ring-1 ring-primary outline-none',
        false: '',
      },
    },
    defaultVariants: { kind: 'default', menuOpen: false },
  },
)

type RowKind = VariantProps<typeof rowStyles>['kind']

/** Shared label styling — flex-1 so the name takes remaining space and truncates. */
const labelStyles = 'flex-1 truncate'

/** Chevron-width spacer reserved on file rows so icons align with folder icons. */
const chevronSpacer = 'w-3.5 shrink-0'

/** True when the file can open in a browse-preview tab (.html / .htm). */
function isHtmlFile(name: string): boolean {
  return /\.html?$/i.test(name)
}

/** Replaces the final path segment — used when committing an inline rename. */
function replaceBasename(path: string, newName: string): string {
  const normalized = path.replace(/\\/g, '/')
  const idx = normalized.lastIndexOf('/')
  if (idx === -1) return newName
  return `${normalized.slice(0, idx + 1)}${newName}`
}

/** Props shared by every recursive tree node for context-menu actions. */
interface TreeActions {
  onFileSelect: (path: string) => void
  onOpenPreview?: (path: string) => void
  onCopyPath?: (path: string) => void
  onCopyRelativePath?: (path: string) => void
  onRename?: (from: string, to: string) => void | Promise<void>
  onDelete?: (path: string, kind: 'file' | 'folder') => void | Promise<void>
  onNewFile?: (parentPath: string) => void | Promise<void>
  onNewFolder?: (parentPath: string) => void | Promise<void>
}

/**
 * Long-press (500ms) → open context menu; move/end/cancel clears the timer.
 * Owns the timer ref inside the hook so refs are never passed to a function
 * during render (react-hooks/refs).
 */
function useLongPressHandlers(onOpen: () => void, enabled = true) {
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const clear = useCallback(() => {
    if (timer.current) {
      clearTimeout(timer.current)
      timer.current = null
    }
  }, [])
  const handlers = useMemo(
    () => ({
      onTouchStart: () => {
        if (!enabled) return
        clear()
        timer.current = setTimeout(onOpen, 500)
      },
      onTouchEnd: clear,
      onTouchMove: clear,
      onTouchCancel: clear,
    }),
    [onOpen, enabled, clear],
  )
  return { handlers, timer, clear }
}

/** Shared Copy / Rename / Delete items for file and folder context menus. */
function PathMenuItems({
  path,
  kind,
  actions,
  onClose,
  onStartRename,
  /** When true and this block has items, insert a separator before them. */
  precedeWithSeparator = false,
}: {
  path: string
  kind: 'file' | 'folder'
  actions: TreeActions
  onClose: () => void
  onStartRename?: () => void
  precedeWithSeparator?: boolean
}) {
  const hasCopy = !!(actions.onCopyPath || actions.onCopyRelativePath)
  const hasMutate = !!(actions.onRename || actions.onDelete)
  if (!hasCopy && !hasMutate) return null

  return (
    <>
      {precedeWithSeparator && <DropdownMenuSeparator />}
      {actions.onCopyPath && (
        <DropdownMenuItem
          onSelect={() => {
            actions.onCopyPath?.(path)
            onClose()
          }}
        >
          <Copy className="w-3.5 h-3.5" />
          Copy Path
        </DropdownMenuItem>
      )}
      {actions.onCopyRelativePath && (
        <DropdownMenuItem
          onSelect={() => {
            actions.onCopyRelativePath?.(path)
            onClose()
          }}
        >
          <Copy className="w-3.5 h-3.5" />
          Copy Relative Path
        </DropdownMenuItem>
      )}
      {hasCopy && hasMutate && <DropdownMenuSeparator />}
      {actions.onRename && onStartRename && (
        <DropdownMenuItem
          onSelect={() => {
            onStartRename()
            onClose()
          }}
        >
          <Pencil className="w-3.5 h-3.5" />
          Rename
        </DropdownMenuItem>
      )}
      {actions.onDelete && (
        <DropdownMenuItem
          variant="destructive"
          onSelect={() => {
            void actions.onDelete?.(path, kind)
            onClose()
          }}
        >
          <Trash2 className="w-3.5 h-3.5" />
          Delete
        </DropdownMenuItem>
      )}
    </>
  )
}

/**
 * Recursive file tree node (Blueprint Sec 17 — file explorer).
 * Supports expand/collapse, unsaved-change indicators, active file highlight,
 * and a right-click / long-press context menu on every file and folder row.
 *
 * Indentation is produced by nesting each expanded folder's children inside a
 * `pl-[20px]` wrapper rather than computing a per-node margin. Each level of
 * recursion adds 20px (the chevron w-3.5 + gap-1.5 footprint), so depth
 * accumulates naturally and works for arbitrarily deep trees, and nested
 * icons line up under their parent folder's icon. This avoids
 * dynamically-constructed Tailwind classes (e.g. `ml-${depth * 4}`), which
 * the JIT compiler cannot detect and therefore never generates — that was
 * the root cause of nested children rendering at the wrong indent level.
 */
function TreeNode({
  node,
  expandedPaths,
  onToggleExpand,
  menuPath,
  setMenuPath,
  renamingPath,
  setRenamingPath,
  ...actions
}: {
  node: FileTreeNode
  expandedPaths: Set<string>
  onToggleExpand: (path: string) => void
  menuPath: string | null
  setMenuPath: (path: string | null) => void
  renamingPath: string | null
  setRenamingPath: (path: string | null) => void
} & TreeActions) {
  const nodePath = node.path || node.name
  const isFolder = node.type === 'folder'
  const isRenaming = renamingPath === nodePath
  const menuOpen = menuPath === nodePath
  const [renameValue, setRenameValue] = useState(node.name)

  const openMenu = useCallback(() => setMenuPath(nodePath), [nodePath, setMenuPath])
  const closeMenu = () => setMenuPath(null)
  // Each row owns its long-press timer; no ref is passed during render.
  const { handlers: touchHandlers } = useLongPressHandlers(openMenu)
  const startRename = () => {
    setRenameValue(node.name)
    setRenamingPath(nodePath)
  }

  const commitRename = () => {
    const trimmed = renameValue.trim()
    setRenamingPath(null)
    if (!trimmed || trimmed === node.name || !actions.onRename) return
    void actions.onRename(nodePath, replaceBasename(nodePath, trimmed))
  }

  const renameInput = (
    <input
      autoFocus
      value={renameValue}
      onChange={(e) => setRenameValue(e.target.value)}
      onBlur={commitRename}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault()
          commitRename()
        } else if (e.key === 'Escape') {
          e.preventDefault()
          setRenameValue(node.name)
          setRenamingPath(null)
        }
      }}
      onClick={(e) => e.stopPropagation()}
      className="flex-1 min-w-0 bg-background text-foreground text-sm rounded px-1 py-0.5 focus:outline-none focus:ring-1 focus:ring-ring"
      aria-label={`Rename ${node.name}`}
    />
  )

  if (isFolder) {
    const isExpanded = expandedPaths.has(nodePath)
    const ChevronIcon = isExpanded ? ChevronDown : ChevronRight
    const FolderIcon = isExpanded ? FolderOpen : Folder

    const folderRow = (
      <div
        className={rowStyles({ kind: 'folder', menuOpen })}
        onClick={() => {
          if (!isRenaming) onToggleExpand(nodePath)
        }}
        onContextMenu={(e) => {
          e.preventDefault()
          e.stopPropagation()
          openMenu()
        }}
        {...touchHandlers}
      >
        <ChevronIcon className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
        <FolderIcon className={cn('w-4 h-4 shrink-0', node.iconColor ?? 'text-primary')} />
        {isRenaming ? renameInput : <span className={labelStyles}>{node.name}</span>}
      </div>
    )

    return (
      <>
        {isRenaming ? (
          folderRow
        ) : (
          <DropdownMenu
            open={menuOpen}
            onOpenChange={(open) => {
              if (!open) closeMenu()
            }}
          >
            <DropdownMenuTrigger asChild>{folderRow}</DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              {actions.onNewFile && (
                <DropdownMenuItem
                  onSelect={() => {
                    void actions.onNewFile?.(nodePath)
                    closeMenu()
                  }}
                >
                  <FilePlus className="w-3.5 h-3.5" />
                  New File
                </DropdownMenuItem>
              )}
              {actions.onNewFolder && (
                <DropdownMenuItem
                  onSelect={() => {
                    void actions.onNewFolder?.(nodePath)
                    closeMenu()
                  }}
                >
                  <FolderPlus className="w-3.5 h-3.5" />
                  New Folder
                </DropdownMenuItem>
              )}
              <PathMenuItems
                path={nodePath}
                kind="folder"
                actions={actions}
                onClose={closeMenu}
                onStartRename={startRename}
                precedeWithSeparator={!!(actions.onNewFile || actions.onNewFolder)}
              />
            </DropdownMenuContent>
          </DropdownMenu>
        )}
        {isExpanded && node.children && (
          // Indent children by the chevron + gap footprint (w-3.5 + gap-1.5 =
          // 20px) so nested icons line up under this folder's icon rather than
          // under its chevron. pl-[20px] is a static value the JIT can detect.
          <div className="pl-[20px]">
            {node.children.map((child) => (
              <TreeNode
                key={child.path || child.name}
                node={child}
                expandedPaths={expandedPaths}
                onToggleExpand={onToggleExpand}
                menuPath={menuPath}
                setMenuPath={setMenuPath}
                renamingPath={renamingPath}
                setRenamingPath={setRenamingPath}
                {...actions}
              />
            ))}
          </div>
        )}
      </>
    )
  }

  // File node. A spacer matching the folder chevron's footprint (w-3.5 + the
  // flex gap that follows it) is reserved so file icons line up with folder
  // icons at the same depth. Without it, files render ~20px left of sibling
  // folders because they lack the chevron glyph.
  const kind: RowKind = node.active ? 'active' : 'default'
  const showPreview = !!actions.onOpenPreview && isHtmlFile(node.name)

  const fileRow = (
    <div
      className={rowStyles({ kind, menuOpen })}
      onClick={() => {
        if (!isRenaming) actions.onFileSelect(nodePath)
      }}
      onContextMenu={(e) => {
        e.preventDefault()
        e.stopPropagation()
        openMenu()
      }}
      {...touchHandlers}
    >
      <span className={chevronSpacer} aria-hidden />
      <FileIcon name={node.name} className="w-4 h-4 shrink-0" />
      {isRenaming ? renameInput : <span className={labelStyles}>{node.name}</span>}
      {node.unsaved && (
        <div title="Unsaved changes" className="shrink-0">
          <Circle className="w-2 h-2 text-primary fill-primary" />
        </div>
      )}
    </div>
  )

  if (isRenaming) return fileRow

  return (
    <DropdownMenu
      open={menuOpen}
      onOpenChange={(open) => {
        if (!open) closeMenu()
      }}
    >
      <DropdownMenuTrigger asChild>{fileRow}</DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        <DropdownMenuItem
          onSelect={() => {
            actions.onFileSelect(nodePath)
            closeMenu()
          }}
        >
          <File className="w-3.5 h-3.5" />
          Open
        </DropdownMenuItem>
        {showPreview && (
          <DropdownMenuItem
            onSelect={() => {
              actions.onOpenPreview?.(nodePath)
              closeMenu()
            }}
          >
            <Eye className="w-3.5 h-3.5" />
            Open Preview
          </DropdownMenuItem>
        )}
        <PathMenuItems
          path={nodePath}
          kind="file"
          actions={actions}
          onClose={closeMenu}
          onStartRename={startRename}
          precedeWithSeparator
        />
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** localStorage key for persisting expanded folder paths per workspace. */
function expandedStorageKey(workspaceId: string): string {
  return `lai:tree-expanded:${workspaceId}`
}

/**
 * Loads the persisted expanded-folder set from localStorage. Returns an empty
 * set if nothing is stored (first visit or cleared storage). All folders are
 * collapsed by default — the user expands what they want, and that choice
 * survives reloads.
 */
function loadExpandedPaths(workspaceId: string | null): Set<string> {
  if (!workspaceId) return new Set()
  try {
    const raw = localStorage.getItem(expandedStorageKey(workspaceId))
    if (!raw) return new Set()
    const arr = JSON.parse(raw) as string[]
    return new Set(Array.isArray(arr) ? arr : [])
  } catch {
    return new Set()
  }
}

/** Persists the expanded-folder set to localStorage for the given workspace. */
function saveExpandedPaths(workspaceId: string | null, expanded: Set<string>): void {
  if (!workspaceId) return
  try {
    localStorage.setItem(expandedStorageKey(workspaceId), JSON.stringify([...expanded]))
  } catch {
    // Ignore quota / serialization errors — persistence is best-effort.
  }
}

/** Sentinel menuPath for the empty explorer background (workspace root). */
const ROOT_MENU_KEY = ''

/** File tree container — renders the workspace file explorer. */
export function FileTree({
  nodes,
  onFileSelect,
  onOpenPreview,
  onCopyPath,
  onCopyRelativePath,
  onRename,
  onDelete,
  onNewFile,
  onNewFolder,
  workspaceId = null,
}: {
  nodes: FileTreeNode[]
  onFileSelect: (path: string) => void
  /** Opens a browse-preview tab for an HTML entry point (context menu). */
  onOpenPreview?: (path: string) => void
  onCopyPath?: (path: string) => void
  onCopyRelativePath?: (path: string) => void
  onRename?: (from: string, to: string) => void | Promise<void>
  onDelete?: (path: string, kind: 'file' | 'folder') => void | Promise<void>
  onNewFile?: (parentPath: string) => void | Promise<void>
  onNewFolder?: (parentPath: string) => void | Promise<void>
  /** Active workspace ID — used to key localStorage persistence of expanded folders. */
  workspaceId?: string | null
}) {
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(() => loadExpandedPaths(workspaceId))
  const [menuPath, setMenuPath] = useState<string | null>(null)
  const [renamingPath, setRenamingPath] = useState<string | null>(null)

  // Recompute the expanded set when the workspace changes (e.g. workspace switch
  // changes workspaceId). Without this, paths from the previous workspace linger
  // and folders the user expanded in the new workspace don't appear. Uses the
  // "adjust state during render" pattern (React docs) instead of setState-in-
  // effect to avoid cascading renders and the react-hooks/set-state-in-effect
  // rule. The workspaceId is a stable string, so this only fires on an actual
  // workspace change, not on every file-tree refresh.
  const [prevWorkspaceId, setPrevWorkspaceId] = useState(workspaceId)
  if (workspaceId !== prevWorkspaceId) {
    setPrevWorkspaceId(workspaceId)
    setExpandedPaths(loadExpandedPaths(workspaceId))
    setMenuPath(null)
    setRenamingPath(null)
  }

  const handleToggleExpand = (path: string) => {
    setExpandedPaths((prev) => {
      const next = new Set(prev)
      if (next.has(path)) {
        next.delete(path)
      } else {
        next.add(path)
      }
      saveExpandedPaths(workspaceId, next)
      return next
    })
  }

  /** Ensures a folder is expanded after creating a child under it. */
  const ensureExpanded = (path: string) => {
    if (!path) return // workspace root — nothing to expand
    setExpandedPaths((prev) => {
      if (prev.has(path)) return prev
      const next = new Set(prev)
      next.add(path)
      saveExpandedPaths(workspaceId, next)
      return next
    })
  }

  const wrapNewFile = onNewFile
    ? async (parentPath: string) => {
        ensureExpanded(parentPath)
        await onNewFile(parentPath)
      }
    : undefined

  const wrapNewFolder = onNewFolder
    ? async (parentPath: string) => {
        ensureExpanded(parentPath)
        await onNewFolder(parentPath)
      }
    : undefined

  const actions: TreeActions = {
    onFileSelect,
    onOpenPreview,
    onCopyPath,
    onCopyRelativePath,
    onRename,
    onDelete,
    onNewFile: wrapNewFile,
    onNewFolder: wrapNewFolder,
  }

  const openRootMenu = useCallback(() => setMenuPath(ROOT_MENU_KEY), [])
  const rootMenuOpen = menuPath === ROOT_MENU_KEY
  const showRootCreate = !!(onNewFile || onNewFolder)
  const { handlers: rootTouchHandlers } = useLongPressHandlers(openRootMenu, showRootCreate)

  /** Empty-area fill: right-click / long-press → New File / New Folder at root. */
  const backgroundFill = showRootCreate ? (
    <DropdownMenu
      open={rootMenuOpen}
      onOpenChange={(open) => {
        if (!open) setMenuPath(null)
      }}
    >
      <DropdownMenuTrigger asChild>
        <div
          className="flex-1 min-h-12 outline-none"
          aria-label="Explorer empty area"
          onContextMenu={(e) => {
            e.preventDefault()
            e.stopPropagation()
            openRootMenu()
          }}
          {...rootTouchHandlers}
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {onNewFile && (
          <DropdownMenuItem
            onSelect={() => {
              void wrapNewFile?.(ROOT_MENU_KEY)
              setMenuPath(null)
            }}
          >
            <FilePlus className="w-3.5 h-3.5" />
            New File
          </DropdownMenuItem>
        )}
        {onNewFolder && (
          <DropdownMenuItem
            onSelect={() => {
              void wrapNewFolder?.(ROOT_MENU_KEY)
              setMenuPath(null)
            }}
          >
            <FolderPlus className="w-3.5 h-3.5" />
            New Folder
          </DropdownMenuItem>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  ) : (
    <div className="flex-1 min-h-12" aria-hidden />
  )

  return (
    <div className="flex flex-1 flex-col min-h-0 overflow-y-auto px-2 pb-2 text-sm">
      {nodes.map((node) => (
        <TreeNode
          key={node.path || node.name}
          node={node}
          expandedPaths={expandedPaths}
          onToggleExpand={handleToggleExpand}
          menuPath={menuPath}
          setMenuPath={setMenuPath}
          renamingPath={renamingPath}
          setRenamingPath={setRenamingPath}
          {...actions}
        />
      ))}
      {backgroundFill}
    </div>
  )
}
