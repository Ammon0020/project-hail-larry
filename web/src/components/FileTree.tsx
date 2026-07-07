import { useState } from 'react'
import {
  ChevronRight,
  ChevronDown,
  Folder,
  FolderOpen,
  FileCode,
  FileText,
  Circle,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import type { FileTreeNode } from '@/types'

/** Maps icon names from mock data to Lucide React components. */
const iconMap: Record<string, typeof Folder> = {
  folder: Folder,
  'folder-open': FolderOpen,
  'file-code': FileCode,
  'file-text': FileText,
}

/**
 * Recursive file tree node (Blueprint Sec 17 — file explorer).
 * Supports expand/collapse, unsaved-change indicators, and active file highlight.
 *
 * Indentation is produced by nesting each expanded folder's children inside a
 * `pl-4` wrapper rather than computing a per-node margin. Each level of
 * recursion adds one `pl-4` (16px), so depth accumulates naturally and works
 * for arbitrarily deep trees. This avoids dynamically-constructed Tailwind
 * classes (e.g. `ml-${depth * 4}`), which the JIT compiler cannot detect and
 * therefore never generates — that was the root cause of nested children
 * rendering at the wrong indent level.
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
          className="flex items-center gap-1.5 p-1 rounded cursor-pointer hover:bg-accent text-foreground"
          onClick={() => onToggleExpand(nodePath)}
        >
          <ChevronIcon className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
          <FolderIcon className={cn('w-4 h-4 shrink-0', node.iconColor ?? 'text-primary')} />
          {node.name}
        </div>
        {isExpanded && node.children && (
          <div className="pl-4">
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

  // File node
  const Icon = iconMap[node.icon ?? 'file-text'] ?? FileText

  if (node.active) {
    return (
      <div
        className="flex items-center justify-between p-1 rounded cursor-pointer bg-primary/10 text-primary border-l-2 border-primary"
        onClick={() => onFileSelect(nodePath)}
      >
        <div className="flex items-center gap-1.5">
          <Icon className={cn('w-4 h-4 shrink-0', node.iconColor ?? 'text-muted-foreground')} />
          {node.name}
        </div>
        {node.unsaved && (
          <div title="Unsaved changes">
            <Circle className="w-2 h-2 text-primary fill-primary" />
          </div>
        )}
      </div>
    )
  }

  return (
    <div
      className="flex items-center gap-1.5 p-1 rounded cursor-pointer hover:bg-accent text-muted-foreground"
      onClick={() => onFileSelect(nodePath)}
    >
      <Icon className={cn('w-4 h-4 shrink-0', node.iconColor ?? 'text-muted-foreground')} />
      {node.name}
    </div>
  )
}

/** Compute initial expanded set: root-level folders and any nodes with expanded=true. */
function getInitialExpanded(nodes: FileTreeNode[]): Set<string> {
  const expanded = new Set<string>()
  for (const node of nodes) {
    if (node.type === 'folder') {
      const nodePath = node.path || node.name
      // Root-level folders start expanded
      expanded.add(nodePath)
      // Also expand children that have expanded=true
      if (node.children) {
        collectExpanded(node.children, expanded)
      }
    }
  }
  return expanded
}

function collectExpanded(nodes: FileTreeNode[], expanded: Set<string>): void {
  for (const node of nodes) {
    if (node.type === 'folder' && node.expanded) {
      expanded.add(node.path || node.name)
      if (node.children) {
        collectExpanded(node.children, expanded)
      }
    }
  }
}

/** File tree container — renders the workspace file explorer. */
export function FileTree({
  nodes,
  onFileSelect,
}: {
  nodes: FileTreeNode[]
  onFileSelect: (path: string) => void
}) {
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(() => getInitialExpanded(nodes))

  // Recompute the expanded set when the tree changes (e.g. workspace switch
  // rebuilds `nodes` from backend.fileTree). Without this, paths from the
  // previous workspace linger and root folders that should default to
  // expanded stay collapsed. Uses the "adjust state during render" pattern
  // (React docs) instead of setState-in-effect to avoid cascading renders and
  // the react-hooks/set-state-in-effect rule. A JSON signature guards against
  // unstable referential identity while still re-running when the tree content
  // changes.
  const [prevSignature, setPrevSignature] = useState(() => JSON.stringify(nodes))
  const currentSignature = JSON.stringify(nodes)
  if (currentSignature !== prevSignature) {
    setPrevSignature(currentSignature)
    setExpandedPaths(getInitialExpanded(nodes))
  }

  const handleToggleExpand = (path: string) => {
    setExpandedPaths((prev) => {
      const next = new Set(prev)
      if (next.has(path)) {
        next.delete(path)
      } else {
        next.add(path)
      }
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
