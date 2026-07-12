import { useState } from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import {
  ChevronRight,
  ChevronDown,
  Folder,
  FolderOpen,
  Circle,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { FileIcon } from '@/lib/fileIcon'
import type { FileTreeNode } from '@/types'

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
    },
    defaultVariants: { kind: 'default' },
  },
)

type RowKind = VariantProps<typeof rowStyles>['kind']

/** Shared label styling — flex-1 so the name takes remaining space and truncates. */
const labelStyles = 'flex-1 truncate'

/** Chevron-width spacer reserved on file rows so icons align with folder icons. */
const chevronSpacer = 'w-3.5 shrink-0'

/**
 * Recursive file tree node (Blueprint Sec 17 — file explorer).
 * Supports expand/collapse, unsaved-change indicators, and active file highlight.
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
  onFileSelect,
}: {
  node: FileTreeNode
  expandedPaths: Set<string>
  onToggleExpand: (path: string) => void
  onFileSelect: (path: string) => void
}) {
  const nodePath = node.path || node.name

  if (node.type === 'folder') {
    const isExpanded = expandedPaths.has(nodePath)
    const ChevronIcon = isExpanded ? ChevronDown : ChevronRight
    const FolderIcon = isExpanded ? FolderOpen : Folder
    return (
      <>
        <div
          className={rowStyles({ kind: 'folder' })}
          onClick={() => onToggleExpand(nodePath)}
        >
          <ChevronIcon className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
          <FolderIcon className={cn('w-4 h-4 shrink-0', node.iconColor ?? 'text-primary')} />
          <span className={labelStyles}>{node.name}</span>
        </div>
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
                onFileSelect={onFileSelect}
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

  return (
    <div
      className={rowStyles({ kind })}
      onClick={() => onFileSelect(nodePath)}
    >
      <span className={chevronSpacer} aria-hidden />
      <FileIcon name={node.name} className="w-4 h-4 shrink-0" />
      <span className={labelStyles}>{node.name}</span>
      {node.unsaved && (
        <div title="Unsaved changes" className="shrink-0">
          <Circle className="w-2 h-2 text-primary fill-primary" />
        </div>
      )}
    </div>
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

/** File tree container — renders the workspace file explorer. */
export function FileTree({
  nodes,
  onFileSelect,
  workspaceId = null,
}: {
  nodes: FileTreeNode[]
  onFileSelect: (path: string) => void
  /** Active workspace ID — used to key localStorage persistence of expanded folders. */
  workspaceId?: string | null
}) {
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(() => loadExpandedPaths(workspaceId))

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

  return (
    <div className="flex-1 overflow-y-auto px-2 pb-2 text-sm">
      {nodes.map((node) => (
        <TreeNode
          key={node.path || node.name}
          node={node}
          expandedPaths={expandedPaths}
          onToggleExpand={handleToggleExpand}
          onFileSelect={onFileSelect}
        />
      ))}
    </div>
  )
}
