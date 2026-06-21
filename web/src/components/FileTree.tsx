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
 */
function TreeNode({
  node,
  depth,
  expandedPaths,
  onToggleExpand,
  onFileSelect,
}: {
  node: FileTreeNode
  depth: number
  expandedPaths: Set<string>
  onToggleExpand: (path: string) => void
  onFileSelect: (path: string) => void
}) {
  const indent = depth > 0 ? `ml-${depth * 4}` : ''
  const nodePath = node.path || node.name

  if (node.type === 'folder') {
    const isExpanded = expandedPaths.has(nodePath)
    const ChevronIcon = isExpanded ? ChevronDown : ChevronRight
    const FolderIcon = isExpanded ? FolderOpen : Folder
    return (
      <>
        <div
          className={cn('flex items-center gap-1.5 p-1 rounded cursor-pointer hover:bg-gray-800/50 text-gray-300', indent)}
          onClick={() => onToggleExpand(nodePath)}
        >
          <ChevronIcon className="w-3.5 h-3.5 text-gray-500 shrink-0" />
          <FolderIcon className={cn('w-4 h-4 shrink-0', node.iconColor ?? 'text-blue-400')} />
          {node.name}
        </div>
        {isExpanded && node.children && (
          <>{node.children.map((child) => (
            <TreeNode
              key={child.path || child.name}
              node={child}
              depth={depth + 1}
              expandedPaths={expandedPaths}
              onToggleExpand={onToggleExpand}
              onFileSelect={onFileSelect}
            />
          ))}</>
        )}
      </>
    )
  }

  // File node
  const Icon = iconMap[node.icon ?? 'file-text'] ?? FileText

  if (node.active) {
    return (
      <div
        className={cn('flex items-center justify-between p-1 rounded cursor-pointer bg-blue-600/10 text-blue-300 border-l-2 border-blue-500', indent)}
        onClick={() => onFileSelect(nodePath)}
      >
        <div className="flex items-center gap-1.5">
          <Icon className={cn('w-4 h-4 shrink-0', node.iconColor ?? 'text-gray-400')} />
          {node.name}
        </div>
        {node.unsaved && (
          <div title="Unsaved changes">
            <Circle className="w-2 h-2 text-blue-400 fill-blue-400" />
          </div>
        )}
      </div>
    )
  }

  return (
    <div
      className={cn('flex items-center gap-1.5 p-1 rounded cursor-pointer hover:bg-gray-800/50 text-gray-400', indent)}
      onClick={() => onFileSelect(nodePath)}
    >
      <Icon className={cn('w-4 h-4 shrink-0', node.iconColor ?? 'text-gray-400')} />
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
          depth={0}
          expandedPaths={expandedPaths}
          onToggleExpand={handleToggleExpand}
          onFileSelect={onFileSelect}
        />
      ))}
    </div>
  )
}
