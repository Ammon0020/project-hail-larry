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
function TreeNode({ node, depth }: { node: FileTreeNode; depth: number }) {
  const indent = depth > 0 ? `ml-${depth * 4}` : ''

  if (node.type === 'folder') {
    const ChevronIcon = node.expanded ? ChevronDown : ChevronRight
    const FolderIcon = node.expanded ? FolderOpen : Folder
    return (
      <>
        <div className={cn('flex items-center gap-1.5 p-1 rounded cursor-pointer hover:bg-gray-800/50 text-gray-300', indent)}>
          <ChevronIcon className="w-3.5 h-3.5 text-gray-500 shrink-0" />
          <FolderIcon className={cn('w-4 h-4 shrink-0', node.iconColor ?? 'text-blue-400')} />
          {node.name}
        </div>
        {node.expanded && node.children && (
          <>{node.children.map((child) => <TreeNode key={child.name} node={child} depth={depth + 1} />)}</>
        )}
      </>
    )
  }

  // File node
  const Icon = iconMap[node.icon ?? 'file-text'] ?? FileText

  if (node.active) {
    return (
      <div className={cn('flex items-center justify-between p-1 rounded cursor-pointer bg-blue-600/10 text-blue-300 border-l-2 border-blue-500', indent)}>
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
    <div className={cn('flex items-center gap-1.5 p-1 rounded cursor-pointer hover:bg-gray-800/50 text-gray-400', indent)}>
      <Icon className={cn('w-4 h-4 shrink-0', node.iconColor ?? 'text-gray-400')} />
      {node.name}
    </div>
  )
}

/** File tree container — renders the workspace file explorer. */
export function FileTree({ nodes }: { nodes: FileTreeNode[] }) {
  return (
    <div className="flex-1 overflow-y-auto px-2 pb-2 text-sm">
      {nodes.map((node) => <TreeNode key={node.name} node={node} depth={0} />)}
    </div>
  )
}
