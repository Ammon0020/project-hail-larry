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

interface FlatFile { name: string; path: string }

interface ScoredFile extends FlatFile {
  namePositions: Set<number>
  pathPositions: Set<number>
  score: number
}

interface ScoredCommand extends Command {
  labelPositions: Set<number>
  score: number
}

function flattenFiles(nodes: FileTreeNode[]): FlatFile[] {
  const result: FlatFile[] = []
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

/**
 * Fuzzy subsequence match: returns matched character positions and a score
 * if every query character appears in order (but not necessarily contiguously)
 * in the target. Higher scores indicate better matches (consecutive chars,
 * early matches, and word-boundary matches get bonuses). Returns null when
 * the query is not a subsequence of the target.
 */
function fuzzyMatch(query: string, target: string): { positions: number[]; score: number } | null {
  if (!query) return { positions: [], score: 0 }
  const q = query.toLowerCase()
  const t = target.toLowerCase()
  const positions: number[] = []
  let score = 0
  let consecutive = 0
  let qi = 0

  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      positions.push(ti)
      consecutive++
      score += consecutive * 5
      if (ti === 0 || t[ti - 1] === '/' || t[ti - 1] === '_' || t[ti - 1] === '-' || t[ti - 1] === '.') {
        score += 10
      }
      if (ti === qi) score += 5
      qi++
    } else {
      consecutive = 0
    }
  }

  if (qi !== q.length) return null
  score -= positions[0] * 2
  return { positions, score }
}

/** Renders text with matched character runs highlighted in text-primary. */
function HighlightedText({ text, positions }: { text: string; positions: Set<number> }) {
  if (positions.size === 0) return text
  const chunks: ReactNode[] = []
  let i = 0
  let key = 0
  while (i < text.length) {
    const isMatch = positions.has(i)
    let j = i + 1
    while (j < text.length && positions.has(j) === isMatch) j++
    const chunk = text.slice(i, j)
    chunks.push(
      isMatch
        ? <span key={key++} className="text-primary">{chunk}</span>
        : chunk,
    )
    i = j
  }
  return <>{chunks}</>
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

  const fileResults = useMemo<ScoredFile[]>(() => {
    if (isCommandMode) return []
    if (!filterText) {
      return files.map((f) => ({ ...f, namePositions: new Set<number>(), pathPositions: new Set<number>(), score: 0 }))
    }
    const matched: ScoredFile[] = []
    for (const f of files) {
      const nameMatch = fuzzyMatch(filterText, f.name)
      if (nameMatch) {
        matched.push({ ...f, namePositions: new Set(nameMatch.positions), pathPositions: new Set(), score: nameMatch.score + 1000 })
        continue
      }
      const pathMatch = fuzzyMatch(filterText, f.path)
      if (pathMatch) {
        matched.push({ ...f, namePositions: new Set(), pathPositions: new Set(pathMatch.positions), score: pathMatch.score })
      }
    }
    matched.sort((a, b) => b.score - a.score)
    return matched
  }, [files, filterText, isCommandMode])

  const commandResults = useMemo<ScoredCommand[]>(() => {
    if (!isCommandMode) return []
    if (!filterText) {
      return commands.map((c) => ({ ...c, labelPositions: new Set<number>(), score: 0 }))
    }
    const matched: ScoredCommand[] = []
    for (const c of commands) {
      const m = fuzzyMatch(filterText, c.label)
      if (m) matched.push({ ...c, labelPositions: new Set(m.positions), score: m.score })
    }
    matched.sort((a, b) => b.score - a.score)
    return matched
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
                <span className="truncate"><HighlightedText text={cmd.label} positions={cmd.labelPositions} /></span>
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
                  <div className="text-sm font-medium truncate">
                    <HighlightedText text={file.name} positions={file.namePositions} />
                  </div>
                  <div className="text-xs text-muted-foreground truncate">
                    <HighlightedText text={file.path} positions={file.pathPositions} />
                  </div>
                </div>
              </div>
            ))
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
