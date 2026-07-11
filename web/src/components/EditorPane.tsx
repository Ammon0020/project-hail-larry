import CodeMirror from '@uiw/react-codemirror'
import { javascript } from '@codemirror/lang-javascript'
import { css } from '@codemirror/lang-css'
import { html } from '@codemirror/lang-html'
import { python } from '@codemirror/lang-python'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import { languages as mdLanguages } from '@codemirror/language-data'
import { oneDark } from '@codemirror/theme-one-dark'
import { search } from '@codemirror/search'
import { autocompletion } from '@codemirror/autocomplete'
import { bracketMatching, foldGutter, indentOnInput, indentUnit } from '@codemirror/language'
import { highlightActiveLine, highlightActiveLineGutter, keymap, EditorView, drawSelection, highlightSpecialChars, rectangularSelection, crosshairCursor } from '@codemirror/view'
import { defaultKeymap, historyKeymap, indentWithTab } from '@codemirror/commands'
import { Prec, EditorSelection } from '@codemirror/state'
import { FileCode, Circle, X, GitCompare, Save, GitBranch, CircleAlert, TriangleAlert, FileText, ChevronLeft, ChevronRight, WrapText, RefreshCw } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { cn } from '@/lib/utils'
import type { Extension } from '@codemirror/state'
import type { Tab } from '@/types'

/**
 * Editor pane — tabbed CodeMirror 6 with diff/save and status bar
 * (Blueprint Sec 17 — center editor).
 *
 * In production, file content arrives via FileRevisionUpdated events.
 * Saves use expectedRevision for three-way merge conflict detection (Sec 14).
 */
export function EditorPane({
  tabs,
  activeTabId,
  visible,
  onTabSelect,
  onTabClose,
  onSave,
  onContentChange,
  onReloadTab,
  onSelectionChange,
  scrollToLine,
}: {
  tabs: Tab[]
  activeTabId: string | null
  visible: boolean
  onTabSelect: (id: string) => void
  onTabClose: (id: string) => void
  onSave: () => void
  onContentChange: (content: string) => void
  /** Reloads a tab's content from disk, discarding local edits. Used by the
   *  "changed on disk" banner's Reload action. */
  onReloadTab?: (id: string) => void
  /** Reports the user's current text selection to the parent so it can be
   *  sent to the backend as a resource block (ACP spec item 1.3). Called
   *  with undefined when the selection is empty (a simple cursor). */
  onSelectionChange?: (selection: { path: string; startLine: number; endLine: number; text: string } | undefined) => void
  /** When set, scrolls the editor cursor to this 1-based line and scrolls it
   *  into view. Cleared by the parent after the jump is dispatched so a
   *  subsequent click on the same line re-triggers. */
  scrollToLine?: number | null
}) {
  const activeTab = tabs.find((t) => t.id === activeTabId) || null

  // Line wrapping toggle — persisted across tab switches within the session.
  const [wrap, setWrap] = useState(false)

  // Keep a ref to the latest onSave so the memoized CodeMirror Ctrl+S keybinding
  // always calls the fresh closure. Without this, the keybinding captures the
  // onSave from when the tab was first opened (useMemo deps don't include onSave),
  // so it would save the ORIGINAL content — silently overwriting the user's edits
  // when both the window-level handler and this stale keybinding fire on Ctrl+S.
  const onSaveRef = useRef(onSave)
  useEffect(() => {
    onSaveRef.current = onSave
  }, [onSave])

  // Ref mirror of onSelectionChange so the memoized updateListener extension
  // always invokes the latest callback without reconfiguring the editor (which
  // would reset cursor/scroll state). Read only inside the updateListener
  // callback (an event handler), never during render.
  const onSelectionChangeRef = useRef(onSelectionChange)
  useEffect(() => {
    onSelectionChangeRef.current = onSelectionChange
  }, [onSelectionChange])

  // Hold the CodeMirror EditorView instance so we can imperatively dispatch
  // selection/scroll transactions (e.g. jump-to-line from search results).
  // Populated via the onCreateEditor callback from @uiw/react-codemirror.
  const editorViewRef = useRef<EditorView | null>(null)

  // When scrollToLine changes (and is non-null), move the cursor to that line
  // and scroll it into view. The parent clears the value after the jump so a
  // subsequent click on the same line number re-triggers the effect.
  useEffect(() => {
    if (scrollToLine == null) return
    const view = editorViewRef.current
    if (!view) return
    // doc.line takes a 1-based line number and returns a line descriptor whose
    // .from is the start position. Guard against out-of-range line numbers.
    if (scrollToLine < 1 || scrollToLine > view.state.doc.lines) return
    const linePos = view.state.doc.line(scrollToLine).from
    view.dispatch({
      selection: EditorSelection.cursor(linePos),
      scrollIntoView: true,
    })
  }, [scrollToLine])

  // Scroll affordances for the tab bar: show left/right chevrons when the tab
  // list overflows. State is updated from the onScroll handler (an event
  // handler, so setState is allowed) and re-measured via requestAnimationFrame
  // when the tab set changes — the rAF callback defers the DOM read until after
  // layout and keeps setState out of the effect body (react-hooks/set-state-in-effect).
  const scrollRef = useRef<HTMLDivElement>(null)
  const [canScrollLeft, setCanScrollLeft] = useState(false)
  const [canScrollRight, setCanScrollRight] = useState(false)

  const measureScroll = () => {
    const el = scrollRef.current
    if (!el) return
    setCanScrollLeft(el.scrollLeft > 0)
    setCanScrollRight(el.scrollLeft < el.scrollWidth - el.clientWidth - 1)
  }

  useEffect(() => {
    const id = requestAnimationFrame(measureScroll)
    return () => cancelAnimationFrame(id)
    // Re-measure whenever the number of tabs changes (add/remove).
  }, [tabs.length])

  const scrollByTabs = (delta: number) => {
    const el = scrollRef.current
    if (!el) return
    el.scrollBy({ left: delta, behavior: 'smooth' })
  }

  /**
   * Resolve a CodeMirror language extension from the tab's language hint.
   *
   * The `language` field on a Tab may be either a canonical name
   * (e.g. "javascript", "python") or a file extension without the dot
   * (e.g. "tsx", "css"). Unknown or empty values default to JavaScript so
   * the editor always has syntax highlighting and bracket handling.
   */
  const getLanguageExtension = (lang: string): Extension[] => {
    const normalized = lang.toLowerCase()
    if (['javascript', 'js', 'jsx', 'ts', 'tsx', 'mjs', 'cjs'].includes(normalized)) {
      return [
        javascript({
          jsx: normalized === 'jsx' || normalized === 'tsx',
          typescript: normalized === 'ts' || normalized === 'tsx',
        }),
      ]
    }
    if (['css', 'scss', 'less'].includes(normalized)) {
      return [css()]
    }
    if (['html', 'htm', 'xml', 'svg'].includes(normalized)) {
      return [html()]
    }
    if (['python', 'py', 'pyw'].includes(normalized)) {
      return [python()]
    }
    if (['markdown', 'md', 'mdx', 'mdown', 'markdown'].includes(normalized)) {
      // markdown() provides syntax highlighting for markdown structure.
      // markdownLanguage + mdLanguages enables nested code block highlighting
      // (e.g. ```js, ```python) via lazy language loading.
      return [markdown({ base: markdownLanguage, codeLanguages: mdLanguages })]
    }
    // Default to JavaScript for unknown/no extension so the editor is never bare.
    return [javascript({ jsx: false, typescript: false })]
  }

  /**
   * Build the full CodeMirror extension list for a tab.
   *
   * Layers, in order:
   *  1. Language support (auto-detected from the tab's language/extension).
   *  2. Full-height theme — makes .cm-editor and .cm-scroller fill the
   *     container so the user can click below the last line of content
   *     (without this, CodeMirror only occupies the height of its content).
   *  3. Search panel (Ctrl+F) — @codemirror/search.
   *  4. Autocompletion — @codemirror/autocomplete.
   *  5. Language-level: bracket matching, fold gutter, indent-on-input,
   *     2-space indent unit.
   *  6. View-level: active line + gutter highlight, draw selection,
   *     highlight special chars, rectangular selection, crosshair cursor.
   *  7. Standard keybindings: defaultKeymap + historyKeymap + indentWithTab.
   *  8. Line wrapping (conditional on the `wrap` toggle).
   *  9. Ctrl+S keybinding at the highest precedence so it overrides the
   *     browser's default "Save Page" behavior and routes to `onSave`.
   *
   * The basicSetup on the <CodeMirror> component provides line numbers,
   * closeBrackets, and other conveniences; the extensions below supplement it
   * with the features that make it feel like a real editor.
   */
  const getExtensions = (lang: string, tabPath: string): Extension[] => {
    const exts: Extension[] = [
      ...getLanguageExtension(lang),

      // Full-height theme: make the editor fill its container so clicking
      // below the last line works (places the cursor at the end). The height
      // chain must be: parent (fixed flex height) → wrapper (100%) →
      // .cm-editor (100%) → .cm-scroller (100%, overflow:auto) →
      // .cm-content (minHeight:100%). Without every link, the editor only
      // takes the height of its content and the area below is dead space.
      EditorView.theme({
        '&': { height: '100%', backgroundColor: 'transparent' },
        '.cm-scroller': { overflow: 'auto' },
        '.cm-content': { minHeight: '100%', paddingBottom: '50vh' },
        '.cm-gutters': { minHeight: '100%' },
      }),

      // Search (Ctrl+F)
      search(),

      // Autocompletion
      autocompletion(),

      // Language-level features
      bracketMatching(),
      foldGutter(),
      indentOnInput(),
      indentUnit.of('  '),

      // View-level features
      highlightActiveLine(),
      highlightActiveLineGutter(),
      drawSelection(),
      highlightSpecialChars(),
      rectangularSelection(),
      crosshairCursor(),

      // Standard keybindings: default + history + tab-to-indent
      keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),

      // Selection listener — reports the current editor selection to the
      // parent (App) so it can be sent to the backend as a resource block
      // (ACP spec item 1.3). Uses the ref so the extension can be memoized
      // without reconfiguring the editor on every parent render. The active
      // tab's path is captured in the closure (passed via getExtensions) so
      // the emitted selection carries the file it belongs to.
      EditorView.updateListener.of((viewUpdate) => {
        if (!viewUpdate.selectionSet && !viewUpdate.docChanged) return
        const cb = onSelectionChangeRef.current
        if (!cb) return
        const { state } = viewUpdate
        const sel = state.selection.main
        if (sel.from === sel.to) {
          // Empty selection (just a cursor) — clear any prior selection.
          cb(undefined)
          return
        }
        const startLine = state.doc.lineAt(sel.from).number
        const endLine = state.doc.lineAt(sel.to).number
        const text = state.sliceDoc(sel.from, sel.to)
        cb({ path: tabPath, startLine, endLine, text })
      }),
    ]

    if (wrap) {
      exts.push(EditorView.lineWrapping)
    }

    // Ctrl+S → save. Highest precedence so the browser does not intercept it.
    exts.push(
      Prec.highest(
        keymap.of([
          {
            key: 'Mod-s',
            preventDefault: true,
            run: () => {
              onSaveRef.current()
              return true
            },
          },
        ]),
      ),
    )

    return exts
  }

  // Memoize extensions per active tab + wrap state so CodeMirror does not
  // reconfigure on every render (which would reset cursor/scroll state).
  // The onSaveRef is read only inside the keybinding's run() callback (an event
  // handler), never during render, so it is safe to reference here.
  const extensions = useMemo(
    // eslint-disable-next-line react-hooks/refs -- ref is only read in the keybinding/updateListener event handlers, not during render
    () => (activeTab ? getExtensions(activeTab.language, activeTab.path) : []),
    // getExtensions depends on `wrap` and the active tab's language/path.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [activeTab?.language, activeTab?.path, wrap, activeTab?.id],
  )

  return (
    <main
      className={cn(
        'flex-1 flex flex-col min-w-0 h-full bg-editor relative pb-16 lg:pb-0',
        visible ? 'flex' : 'hidden',
      )}
    >
      {/* Tab Bar */}
      <div className="flex items-center bg-panel border-b border-background shrink-0 h-9">
        {canScrollLeft && (
          <button
            type="button"
            aria-label="Scroll tabs left"
            onClick={() => scrollByTabs(-150)}
            className="flex items-center justify-center w-5 h-9 shrink-0 text-muted-foreground hover:text-foreground hover:bg-editor/50 transition"
          >
            <ChevronLeft className="w-4 h-4" />
          </button>
        )}
        <div
          ref={scrollRef}
          onScroll={measureScroll}
          onWheel={(e) => {
            // Translate vertical scrollwheel into horizontal tab scrolling.
            // The container only scrolls on x, so without this the wheel
            // does nothing when hovering the tab bar.
            if (e.deltaY !== 0) {
              scrollRef.current?.scrollBy({ left: e.deltaY, behavior: 'smooth' })
            }
          }}
          className="flex overflow-x-auto tab-scrollbar"
        >
          {tabs.map((tab) => {
            const isActive = tab.id === activeTabId
            return (
              <div
                key={tab.id}
                className={cn(
                  'flex items-center gap-2 px-3 h-9 text-sm shrink-0 border-r border-background cursor-pointer',
                  isActive
                    ? 'bg-editor text-foreground border-t-2 border-primary'
                    : 'bg-panel text-muted-foreground hover:bg-editor/50 transition',
                )}
                onClick={() => onTabSelect(tab.id)}
              >
                <FileCode className="w-3.5 h-3.5 text-yellow-400" />
                {tab.name}
                {tab.unsaved && (
                  <Circle className="w-2 h-2 text-primary fill-primary" />
                )}
                {tab.changedOnDisk && (
                  <>
                    <RefreshCw className="w-3 h-3 text-warning" aria-hidden="true" />
                    <span className="sr-only">Changed on disk</span>
                  </>
                )}
                <X
                  className="w-3.5 h-3.5 text-muted-foreground hover:text-foreground cursor-pointer ml-1"
                  onClick={(e) => {
                    e.stopPropagation()
                    onTabClose(tab.id)
                  }}
                  aria-label={`Close ${tab.name}`}
                  role="button"
                />
              </div>
            )
          })}
        </div>
        {canScrollRight && (
          <button
            type="button"
            aria-label="Scroll tabs right"
            onClick={() => scrollByTabs(150)}
            className="flex items-center justify-center w-5 h-9 shrink-0 text-muted-foreground hover:text-foreground hover:bg-editor/50 transition"
          >
            <ChevronRight className="w-4 h-4" />
          </button>
        )}
        <div className="flex-1" />
        {/* Editor actions: Wrap toggle + Diff + Save (Blueprint Sec 14 — file sync) */}
        {activeTab && (
          <div className="hidden md:flex gap-1.5 pr-3 items-center">
            <button
              type="button"
              aria-label="Toggle line wrapping"
              aria-pressed={wrap}
              title="Toggle line wrapping"
              onClick={() => setWrap((w) => !w)}
              className={cn(
                'flex items-center justify-center w-7 h-6 rounded transition',
                wrap
                  ? 'bg-primary text-primary-foreground hover:bg-primary/90'
                  : 'bg-secondary text-secondary-foreground hover:bg-accent',
              )}
            >
              <WrapText className="w-3.5 h-3.5" />
            </button>
            <button className="text-xs font-semibold bg-secondary hover:bg-accent text-secondary-foreground px-2.5 py-1 rounded transition flex items-center gap-1.5">
              <GitCompare className="w-3 h-3" /> Diff
            </button>
            <button
              onClick={onSave}
              className="text-xs font-semibold bg-primary hover:bg-primary/90 text-primary-foreground px-2.5 py-1 rounded flex items-center gap-1.5 transition"
            >
              <Save className="w-3 h-3" /> Save
            </button>
          </div>
        )}
      </div>

      {/* Changed-on-disk banner — shown when the active tab's file was modified
          on disk (agent write / external edit) while the user had unsaved
          edits, so its content was NOT auto-refreshed. Offers a Reload that
          discards local edits and fetches the on-disk version. Uses the
          warning semantic token so it adapts to the active theme. */}
      {activeTab?.changedOnDisk && (
        <div className="flex items-center justify-between gap-2 bg-warning/10 border-b border-warning/40 px-3 py-1.5 text-xs text-warning shrink-0">
          <span className="flex items-center gap-1.5">
            <TriangleAlert className="w-3.5 h-3.5" />
            This file changed on disk{activeTab.unsaved ? ' and you have unsaved edits' : ''}.
          </span>
          <button
            type="button"
            onClick={() => onReloadTab?.(activeTab.id)}
            className="flex items-center gap-1 font-medium text-warning bg-warning/15 hover:bg-warning/25 px-2 py-0.5 rounded transition"
          >
            <RefreshCw className="w-3 h-3" aria-hidden="true" /> Reload
          </button>
        </div>
      )}

      {/* CodeMirror 6 Editor or Empty State (Blueprint Sec 17 — CodeMirror 6) */}
      <div className="flex-1 overflow-hidden bg-editor">
        {activeTab ? (
          <CodeMirror
            value={activeTab.content}
            onChange={onContentChange}
            extensions={extensions}
            theme={oneDark}
            height="100%"
            className="text-[13px] h-full"
            onCreateEditor={(view) => {
              editorViewRef.current = view
            }}
            basicSetup={{
              lineNumbers: true,
              foldGutter: true,
              highlightActiveLine: true,
              autocompletion: true,
              bracketMatching: true,
              closeBrackets: true,
              indentOnInput: true,
            }}
          />
        ) : (
          <div className="flex items-center justify-center h-full">
            <div className="text-center text-muted-foreground">
              <FileText className="w-12 h-12 mx-auto mb-3 text-muted-foreground" />
              <p className="text-sm">Open a file from the explorer</p>
            </div>
          </div>
        )}
      </div>

      {/* Status Bar (Blueprint Sec 17) */}
      <div className="flex items-center justify-between bg-status-bar text-white text-[10px] md:text-[11px] px-3 py-0.5 shrink-0">
        <div className="flex items-center gap-3">
          <span className="flex items-center gap-1"><GitBranch className="w-3 h-3" /> main</span>
          <span className="hidden md:flex items-center gap-1"><CircleAlert className="w-3 h-3" /> 0 errors</span>
          <span className="hidden md:flex items-center gap-1"><TriangleAlert className="w-3 h-3" /> 0 warnings</span>
        </div>
        <div className="flex items-center gap-3">
          <span className="hidden md:inline">{activeTab?.language || 'Plain Text'}</span>
          <span className="hidden md:inline">UTF-8</span>
          <span className="hidden md:inline">LF</span>
          <span>Ln 1, Col 1</span>
        </div>
      </div>
    </main>
  )
}
