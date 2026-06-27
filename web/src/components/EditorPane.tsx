import CodeMirror from '@uiw/react-codemirror'
import { javascript } from '@codemirror/lang-javascript'
import { css } from '@codemirror/lang-css'
import { html } from '@codemirror/lang-html'
import { python } from '@codemirror/lang-python'
import { oneDark } from '@codemirror/theme-one-dark'
import { search } from '@codemirror/search'
import { autocompletion } from '@codemirror/autocomplete'
import { bracketMatching, foldGutter, indentOnInput, indentUnit } from '@codemirror/language'
import { highlightActiveLine, highlightActiveLineGutter, keymap, EditorView, drawSelection, highlightSpecialChars, rectangularSelection, crosshairCursor } from '@codemirror/view'
import { defaultKeymap, historyKeymap, indentWithTab } from '@codemirror/commands'
import { Prec } from '@codemirror/state'
import { FileCode, Circle, X, GitCompare, Save, GitBranch, CircleAlert, TriangleAlert, FileText, ChevronLeft, ChevronRight, WrapText } from 'lucide-react'
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
}: {
  tabs: Tab[]
  activeTabId: string | null
  visible: boolean
  onTabSelect: (id: string) => void
  onTabClose: (id: string) => void
  onSave: () => void
  onContentChange: (content: string) => void
}) {
  const activeTab = tabs.find((t) => t.id === activeTabId) || null

  // Line wrapping toggle — persisted across tab switches within the session.
  const [wrap, setWrap] = useState(false)

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
  const getExtensions = (lang: string): Extension[] => {
    const exts: Extension[] = [
      ...getLanguageExtension(lang),

      // Full-height theme: make the editor fill its container so clicking
      // below the last line works (places the cursor at the end). Without
      // this, .cm-editor only takes the height of its content.
      EditorView.theme({
        '&': { height: '100%' },
        '.cm-scroller': { overflow: 'auto' },
        '.cm-content': { minHeight: '100%' },
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
              onSave()
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
  const extensions = useMemo(
    () => (activeTab ? getExtensions(activeTab.language) : []),
    // getExtensions depends on `wrap` and the active tab's language.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [activeTab?.language, wrap, activeTab?.id],
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
            className="flex items-center justify-center w-5 h-9 shrink-0 text-gray-400 hover:text-gray-200 hover:bg-editor/50 transition"
          >
            <ChevronLeft className="w-4 h-4" />
          </button>
        )}
        <div
          ref={scrollRef}
          onScroll={measureScroll}
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
                    ? 'bg-editor text-gray-200 border-t-2 border-blue-500'
                    : 'bg-panel text-gray-500 hover:bg-editor/50 transition',
                )}
                onClick={() => onTabSelect(tab.id)}
              >
                <FileCode className="w-3.5 h-3.5 text-yellow-400" />
                {tab.name}
                {tab.unsaved && (
                  <Circle className="w-2 h-2 text-blue-400 fill-blue-400" />
                )}
                <X
                  className="w-3.5 h-3.5 text-gray-500 hover:text-white cursor-pointer ml-1"
                  onClick={(e) => {
                    e.stopPropagation()
                    onTabClose(tab.id)
                  }}
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
            className="flex items-center justify-center w-5 h-9 shrink-0 text-gray-400 hover:text-gray-200 hover:bg-editor/50 transition"
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
                  ? 'bg-blue-600 text-white hover:bg-blue-500'
                  : 'bg-gray-800 text-gray-300 hover:bg-gray-700',
              )}
            >
              <WrapText className="w-3.5 h-3.5" />
            </button>
            <button className="text-xs font-semibold bg-gray-800 hover:bg-gray-700 text-gray-300 px-2.5 py-1 rounded transition flex items-center gap-1.5">
              <GitCompare className="w-3 h-3" /> Diff
            </button>
            <button
              onClick={onSave}
              className="text-xs font-semibold bg-blue-600 hover:bg-blue-500 text-white px-2.5 py-1 rounded flex items-center gap-1.5 transition"
            >
              <Save className="w-3 h-3" /> Save
            </button>
          </div>
        )}
      </div>

      {/* CodeMirror 6 Editor or Empty State (Blueprint Sec 17 — CodeMirror 6) */}
      <div className="flex-1 overflow-auto bg-editor">
        {activeTab ? (
          <CodeMirror
            value={activeTab.content}
            onChange={onContentChange}
            extensions={extensions}
            theme={oneDark}
            height="100%"
            className="text-[13px]"
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
            <div className="text-center text-gray-500">
              <FileText className="w-12 h-12 mx-auto mb-3 text-gray-600" />
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
