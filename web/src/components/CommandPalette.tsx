import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Search } from 'lucide-react'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { FileIcon } from '@/lib/fileIcon'
import { cn } from '@/lib/utils'
import type { FileTreeNode } from '@/types'

export interface Command {
  id: string
  label: string
  icon?: ReactNode
  action: () => void
}

interface CommandPaletteProps {
  fileTree: FileTreeNode[]
  onFileSelect: (path: string) => void
  commands: Command[]
}

function flattenFiles(nodes: FileTreeNode[]): { name: string; path: string }[] {
  const result: { name: string; path: string }[] = []
  for (const node of nodes) {
    if (node.type === 'file' && node.path) {
      result.push({ name: node.name, path: node.path })
    }
    if (node.children) {
      result.push(...flattenFiles(node.children))
    }
  }
  return result
}

export function CommandPalette({
  fileTree,
  onFileSelect,
  commands,
}: CommandPaletteProps) {
  const [open, setOpen] = useState(false)
  const [prevOpen, setPrevOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handler = () => setOpen(true)
    window.addEventListener('command-palette-open', handler)
    return () => window.removeEventListener('command-palette-open', handler)
  }, [])

  const close = useCallback(() => setOpen(false), [])

  const files = useMemo(() => flattenFiles(fileTree), [fileTree])

  const isCommandMode = query.startsWith('>')
  const filterText = isCommandMode ? query.slice(1).trim().toLowerCase() : query.trim().toLowerCase()

  const fileResults = useMemo(() => {
    if (isCommandMode) return []
    if (!filterText) return files
    return files.filter(
      (f) =>
        f.name.toLowerCase().includes(filterText) ||
        f.path.toLowerCase().includes(filterText),
    )
  }, [files, filterText, isCommandMode])

  const commandResults = useMemo(() => {
    if (!isCommandMode) return []
    if (!filterText) return commands
    return commands.filter((c) => c.label.toLowerCase().includes(filterText))
  }, [commands, filterText, isCommandMode])

  const MAX_RESULTS = 100
  const visibleFiles = fileResults.slice(0, MAX_RESULTS)
  const visibleCommands = commandResults.slice(0, MAX_RESULTS)
  const resultCount = isCommandMode ? visibleCommands.length : visibleFiles.length

  const handleQueryChange = (next: string) => {
    setQuery(next)
    setSelectedIndex(0)
  }

  if (prevOpen !== open) {
    setPrevOpen(open)
    if (open) {
      setQuery('')
      setSelectedIndex(0)
    }
  }

  useEffect(() => {
    if (!open) return
    const t = setTimeout(() => inputRef.current?.focus(), 0)
    return () => clearTimeout(t)
  }, [open])

  useEffect(() => {
    if (!listRef.current) return
    const el = listRef.current.querySelector<HTMLElement>(`[data-idx="${selectedIndex}"]`)
    el?.scrollIntoView({ block: 'nearest' })
  }, [selectedIndex])

  const activate = (index: number) => {
    if (isCommandMode) {
      const cmd = visibleCommands[index]
      if (!cmd) return
      cmd.action()
    } else {
      const file = visibleFiles[index]
      if (!file) return
      onFileSelect(file.path)
    }
    close()
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSelectedIndex((i) => (resultCount === 0 ? 0 : (i + 1) % resultCount))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSelectedIndex((i) => (resultCount === 0 ? 0 : (i - 1 + resultCount) % resultCount))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      activate(selectedIndex)
    }
  }

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!v) close() }}>
      <DialogContent
        showCloseButton={false}
        className="max-w-xl top-[10vh] translate-y-0 left-1/2 -translate-x-1/2 p-0 gap-0"
        onKeyDown={handleKeyDown}
      >
        <DialogTitle className="sr-only">Command Palette</DialogTitle>
        <div className="flex items-center gap-2 px-3 py-2 border-b border-border">
          <Search className="w-4 h-4 shrink-0 text-muted-foreground" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => handleQueryChange(e.target.value)}
            placeholder="Search files by name... (use > for commands)"
            className="flex-1 bg-transparent outline-none text-sm text-foreground placeholder:text-muted-foreground"
            autoFocus
          />
        </div>
        <div ref={listRef} className="max-h-[400px] overflow-y-auto p-1">
          {resultCount === 0 ? (
            <div className="px-3 py-6 text-center text-sm text-muted-foreground">
              No {isCommandMode ? 'commands' : 'files'} found
            </div>
          ) : isCommandMode ? (
            visibleCommands.map((cmd, i) => (
              <div
                key={cmd.id}
                data-idx={i}
                onMouseEnter={() => setSelectedIndex(i)}
                onClick={() => activate(i)}
                className={cn(
                  'flex items-center gap-2 px-3 py-2 rounded cursor-pointer text-sm',
                  i === selectedIndex && 'bg-accent text-accent-foreground',
                )}
              >
                {cmd.icon && <span className="shrink-0">{cmd.icon}</span>}
                <span className="truncate">{cmd.label}</span>
              </div>
            ))
          ) : (
            visibleFiles.map((file, i) => (
              <div
                key={file.path}
                data-idx={i}
                onMouseEnter={() => setSelectedIndex(i)}
                onClick={() => activate(i)}
                className={cn(
                  'flex items-center gap-2 px-3 py-2 rounded cursor-pointer',
                  i === selectedIndex && 'bg-accent text-accent-foreground',
                )}
              >
                <FileIcon name={file.name} className="w-4 h-4 shrink-0" />
                <div className="min-w-0 flex-1">
                  <div className="text-sm font-medium truncate">{file.name}</div>
                  <div className="text-xs text-muted-foreground truncate">{file.path}</div>
                </div>
              </div>
            ))
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
