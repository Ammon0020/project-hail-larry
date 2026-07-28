import { MergeView, unifiedMergeView } from '@codemirror/merge'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView } from '@codemirror/view'
import { EditorState } from '@codemirror/state'
import type { Extension } from '@codemirror/state'
import { useEffect, useRef, useState } from 'react'
import { cn } from '@/lib/utils'
import { languageExtensionsForPath } from '@/lib/language'

/**
 * Structured git diff viewer (S-GIT-DIFF-VIEWER).
 *
 * Renders a side-by-side (`split`) or inline (`unified`) diff between two
 * document snapshots using CodeMirror 6's `@codemirror/merge`. The viewer is
 * read-only — it is for inspecting changes, not editing them.
 *
 * This component is intentionally decoupled from the API: callers pass `base`
 * and `head` strings directly. The editor-tab dispatcher wraps it in
 * `GitDiffTab`, which fetches via `api.getGitDiff`; future callers (e.g. a
 * chat "edited files" popup) can render `GitDiffViewer` directly with their
 * own content without going through the workspace diff endpoint.
 *
 * NOTE: Tests are deferred — the project has no vitest/jest setup yet. When a
 * test runner is added, cover: added file (base empty), modified file (both
 * non-empty), deleted file (head empty), and `truncated: true` banner.
 */

export interface GitDiffViewerProps {
  /** File path relative to the workspace root — used for language detection
   *  and shown in the viewer header. */
  path: string
  /** Original (pre-change) file contents. Empty for an added file. */
  base: string
  /** New (post-change) file contents. Empty for a deleted file. */
  head: string
  /** When true, the backend capped the diff at its size limit — render a
   *  non-blocking banner so the user knows the view is incomplete. */
  truncated?: boolean
  /** Initial render mode. Defaults to `'unified'`. Split collapses to
   *  unified on narrow viewports (see `useEffectiveMode`). */
  mode?: 'unified' | 'split'
}

/** Read-only + non-editable editor extensions shared by both sides. The merge
 *  viewer is for inspection only — `editable: false` hides the caret and
 *  disables input, `readOnly` prevents programmatic edits from extensions. */
const readOnlyExt: Extension[] = [EditorState.readOnly.of(true), EditorView.editable.of(false)]

/** Minimum viewport width (px) below which split mode collapses to unified.
 *  Two side-by-side editors are unreadable on phone-width screens. */
const SPLIT_MIN_WIDTH = 640

/** Reads `lai:fontSize` / `lai:wrap` from localStorage, matching the keys
 *  used by `useEditorSettings` so the diff viewer inherits the user's editor
 *  preferences. Falls back to 14px / no-wrap when unset. */
function readEditorPrefs(): { fontSize: number; wrap: boolean } {
  let fontSize = 14
  let wrap = false
  try {
    const raw = localStorage.getItem('lai:fontSize')
    if (raw) {
      const n = Number(raw)
      if (Number.isFinite(n) && n > 0) fontSize = n
    }
    const w = localStorage.getItem('lai:wrap')
    if (w === 'true') wrap = true
  } catch {
    // localStorage may be unavailable (private mode / SSR) — keep defaults.
  }
  return { fontSize, wrap }
}

/**
 * Pick the effective mode, collapsing `split` → `unified` on narrow
 * viewports so two side-by-side editors don't render unreadably on phones.
 * Re-evaluates on window resize.
 */
function useEffectiveMode(
  requested: 'unified' | 'split',
): { effective: 'unified' | 'split'; isNarrow: boolean } {
  const [isNarrow, setIsNarrow] = useState<boolean>(() =>
    typeof window !== 'undefined' && window.innerWidth < SPLIT_MIN_WIDTH
  )

  useEffect(() => {
    const onResize = () => {
      setIsNarrow(typeof window !== 'undefined' && window.innerWidth < SPLIT_MIN_WIDTH)
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])

  return { effective: requested === 'split' && isNarrow ? 'unified' : requested, isNarrow }
}

export function GitDiffViewer({ path, base, head, truncated, mode = 'unified' }: GitDiffViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  // MergeView (split) or EditorView (unified) — only one is live at a time.
  const viewRef = useRef<MergeView | EditorView | null>(null)
  const [userMode, setUserMode] = useState<'unified' | 'split'>(mode)
  const { effective: effectiveMode, isNarrow } = useEffectiveMode(userMode)

  // (Re)build the merge view whenever the inputs or mode change. The
  // @codemirror/merge MergeView is constructed imperatively (there is no
  // react-codemirror wrapper for it), so we tear down and rebuild on each
  // dependency change. Diffs are read-only and rebuilt on tab switch, so the
  // cost is acceptable; a future optimization could use `reconfigure` for
  // mode-only toggles.
  useEffect(() => {
    const parent = containerRef.current
    if (!parent) return

    const { fontSize, wrap } = readEditorPrefs()
    const langExts = languageExtensionsForPath(path)
    // TODO: theme integration — `oneDark` is hardcoded to match EditorPane's
    // current behavior. When EditorPane adopts the data-theme-aware theme,
    // share that extension here so the diff view follows light/dark mode.
    const themeExts: Extension[] = [
      oneDark,
      EditorView.theme({
        '&': { height: '100%', fontSize: `${fontSize}px` },
        '.cm-scroller': { overflow: 'auto' },
      }),
    ]
    if (wrap) themeExts.push(EditorView.lineWrapping)

    let view: MergeView | EditorView
    if (effectiveMode === 'split') {
      view = new MergeView({
        a: { doc: base, extensions: [...langExts, ...themeExts, ...readOnlyExt] },
        b: { doc: head, extensions: [...langExts, ...themeExts, ...readOnlyExt] },
        parent,
        // Hide revert controls — this is a read-only inspection view, not a
        // 3-way merge tool, so accept/reject buttons would be misleading.
        revertControls: undefined,
      })
    } else {
      // Unified mode: a single editor showing `head`, with `unifiedMergeView`
      // overlaying deleted/inserted gutter markers against `base`.
      view = new EditorView({
        state: EditorState.create({
          doc: head,
          extensions: [
            ...langExts,
            ...themeExts,
            ...readOnlyExt,
            unifiedMergeView({
              original: base,
              // Hide inline accept/reject controls — read-only inspection.
              syntaxHighlightDeletions: true,
            }),
          ],
        }),
        parent,
      })
    }
    viewRef.current = view
    return () => {
      view.destroy()
      if (viewRef.current === view) viewRef.current = null
    }
  }, [base, head, path, effectiveMode])

  return (
    <div className="flex flex-col h-full min-h-0 bg-editor border-border">
      {/* Header: path + mode toggle. Split is disabled on narrow viewports
          where it would collapse to unified — `active` reflects the effective
          rendered mode so the highlight never lies. */}
      <div className="flex items-center justify-between gap-2 px-3 py-1.5 border-b border-border text-xs text-muted-foreground shrink-0">
        <span className="truncate font-mono" title={path}>{path}</span>
        <div className="flex items-center gap-1 shrink-0">
          <ModeButton
            label="Unified"
            active={effectiveMode === 'unified'}
            onClick={() => setUserMode('unified')}
          />
          <ModeButton
            label="Split"
            active={effectiveMode === 'split'}
            disabled={isNarrow}
            title={isNarrow ? 'Split needs a wider screen' : undefined}
            onClick={() => setUserMode('split')}
          />
        </div>
      </div>

      {truncated && (
        <div role="status" aria-live="polite" className="px-3 py-1 text-xs bg-yellow-500/10 text-yellow-600 border-b border-yellow-500/20">
          Diff truncated at size cap.
        </div>
      )}

      {/* The CodeMirror merge view mounts here imperatively. `min-h-0` keeps
          the flex child from overflowing the pane on small viewports. */}
      <div ref={containerRef} className="flex-1 min-h-0 overflow-auto" />
    </div>
  )
}

/** Small toggle button for the Unified/Split mode switch. */
function ModeButton({
  label,
  active,
  disabled,
  title,
  onClick,
}: {
  label: string
  active: boolean
  disabled?: boolean
  title?: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      disabled={disabled}
      title={title}
      onClick={onClick}
      className={cn(
        'px-2 py-0.5 rounded transition',
        disabled && 'opacity-40 cursor-not-allowed',
        active
          ? 'bg-foreground/10 text-foreground font-medium'
          : 'text-muted-foreground hover:bg-foreground/5',
      )}
    >
      {label}
    </button>
  )
}
