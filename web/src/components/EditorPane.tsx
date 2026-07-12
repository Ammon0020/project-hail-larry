import CodeMirror from '@uiw/react-codemirror'
import { javascript } from '@codemirror/lang-javascript'
import { css } from '@codemirror/lang-css'
import { html } from '@codemirror/lang-html'
import { python } from '@codemirror/lang-python'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import { json } from '@codemirror/lang-json'
import { languages as mdLanguages } from '@codemirror/language-data'
import { oneDark } from '@codemirror/theme-one-dark'
import { search } from '@codemirror/search'
import { autocompletion } from '@codemirror/autocomplete'
import { LanguageDescription, bracketMatching, foldGutter, indentOnInput, indentUnit } from '@codemirror/language'
import { highlightActiveLine, highlightActiveLineGutter, keymap, EditorView, drawSelection, highlightSpecialChars, rectangularSelection, crosshairCursor } from '@codemirror/view'
import { defaultKeymap, historyKeymap, indentWithTab } from '@codemirror/commands'
import { Prec, EditorSelection } from '@codemirror/state'
import { GitBranch, CircleAlert, TriangleAlert, FileText, RefreshCw, FileX, ZoomIn, ZoomOut } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { cn } from '@/lib/utils'
import { SettingsPanel } from '@/components/SettingsPanel'
import { TabBar } from './TabBar'
import type { Agent } from '@/types'
import type { Extension } from '@codemirror/state'
import type { LanguageSupport } from '@codemirror/language'
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
  settingsProps,
  hideTabBar = false,
  wrap = false,
  onToggleWrap,
  isDesktop,
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
  /** Props for the SettingsPanel, rendered when the active tab has
   *  kind === 'settings'. Passed through from App.tsx. */
  settingsProps?: {
    agents: Agent[]
    onAddAgent: (a: Agent) => Promise<void>
    onDeleteAgent: (id: string) => Promise<void>
    onAutodetect: () => Promise<Agent[]>
    /** Id of the active chat session, or null when none is open. Threaded
     *  through to the Providers (advanced) section of SettingsPanel. */
    activeSessionId?: string | null
  }
  hideTabBar?: boolean
  wrap?: boolean
  onToggleWrap?: () => void
  /** Whether the viewport is desktop-sized (≥1024px). When false, the editor
   *  applies touch-friendly theme tweaks: larger line height, wider gutters,
   *  bigger font, and disables mouse-only selection modes. */
  isDesktop?: boolean
}) {
  const activeTab = tabs.find((t) => t.id === activeTabId) || null
  const mobile = !(isDesktop ?? true)

  // Editor font size — persisted to localStorage so the user's zoom preference
  // survives reloads. Adjustable via +/- buttons in the status bar. Defaults to
  // 13px on desktop, 15px on mobile for better readability on small screens.
  const [fontSize, setFontSize] = useState<number>(() => {
    const stored = localStorage.getItem('lai:editor-font-size')
    if (stored) return parseInt(stored, 10) || (mobile ? 15 : 13)
    return mobile ? 15 : 13
  })
  useEffect(() => {
    localStorage.setItem('lai:editor-font-size', String(fontSize))
  }, [fontSize])

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

  // Hold the CodeMirror EditorView instances so we can imperatively dispatch
  // selection/scroll transactions (e.g. jump-to-line from search results).
  // Populated via the onCreateEditor callback from @uiw/react-codemirror.
  const editorViewsRef = useRef<Record<string, EditorView>>({})

  // When scrollToLine changes (and is non-null), move the cursor to that line
  // and scroll it into view. The parent clears the value after the jump so a
  // subsequent click on the same line number re-triggers the effect.
  useEffect(() => {
    if (scrollToLine == null || !activeTabId) return
    const view = editorViewsRef.current[activeTabId]
    if (!view) return
    // doc.line takes a 1-based line number and returns a line descriptor whose
    // .from is the start position. Guard against out-of-range line numbers.
    if (scrollToLine < 1 || scrollToLine > view.state.doc.lines) return
    const linePos = view.state.doc.line(scrollToLine).from
    view.dispatch({
      selection: EditorSelection.cursor(linePos),
      scrollIntoView: true,
    })
  }, [scrollToLine, activeTabId])

  // On mobile, when the soft keyboard opens/closes the viewport height changes.
  // Scroll the cursor back into view so the line being edited isn't hidden
  // behind the keyboard. Only active on touch devices.
  useEffect(() => {
    if (!mobile) return
    const onResize = () => {
      if (!activeTabId) return
      const view = editorViewsRef.current[activeTabId]
      if (!view) return
      view.dispatch({
        effects: EditorView.scrollIntoView(view.state.selection.main.head, { y: 'nearest' }),
      })
    }
    window.addEventListener('resize', onResize)
    if (window.visualViewport) {
      window.visualViewport.addEventListener('resize', onResize)
    }
    return () => {
      window.removeEventListener('resize', onResize)
      if (window.visualViewport) {
        window.visualViewport.removeEventListener('resize', onResize)
      }
    }
  }, [mobile, activeTabId])

  /**
   * Resolve a CodeMirror language extension from the tab's language hint.
   *
   * The `language` field on a Tab may be either a canonical name
   * (e.g. "javascript", "python") or a file extension without the dot
   * (e.g. "tsx", "css"). Unknown or empty values default to JavaScript so
   * the editor always has syntax highlighting and bracket handling.
   */
  const [loadedSupports, setLoadedSupports] = useState<Record<string, LanguageSupport>>({})
  const loadingRef = useRef<Set<string>>(new Set())
  function loadLanguage(desc: LanguageDescription) {
    if (desc.support || loadedSupports[desc.name] || loadingRef.current.has(desc.name)) return
    loadingRef.current.add(desc.name)
    desc.load().then((support) => {
      setLoadedSupports((prev) => ({ ...prev, [desc.name]: support }))
    }).catch(() => {
      loadingRef.current.delete(desc.name)
    })
  }

  const getLanguageExtension = (lang: string, tabPath: string): Extension[] => {
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
    if (['json', 'jsonc'].includes(normalized)) {
      return [json()]
    }
    // Default to JavaScript for unknown/no extension so the editor is never bare.
    const filename = tabPath.split(/[\\/]/).pop() || tabPath
    const desc = LanguageDescription.matchFilename(mdLanguages, filename)
    if (desc) {
      if (desc.support) return [desc.support]
      if (loadedSupports[desc.name]) return [loadedSupports[desc.name]]
      loadLanguage(desc)
      return []
    }
    return []
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
      ...getLanguageExtension(lang, tabPath),

      // Full-height theme: make the editor fill its container so clicking
      // below the last line works (places the cursor at the end). The height
      // chain must be: parent (fixed flex height) → wrapper (100%) →
      // .cm-editor (100%) → .cm-scroller (100%, overflow:auto) →
      // .cm-content (minHeight:100%). Without every link, the editor only
      // takes the height of its content and the area below is dead space.
      EditorView.theme({
        '&': {
          height: '100%',
          backgroundColor: 'transparent',
          fontSize: `${fontSize}px`,
        },
        '.cm-scroller': {
          overflow: 'auto',
          ...(mobile ? { WebkitOverflowScrolling: 'touch' as const } : {}),
        },
        '.cm-content': {
          minHeight: '100%',
          paddingBottom: '50vh',
          ...(mobile ? { lineHeight: '28px' } : {}),
        },
        '.cm-gutters': {
          minHeight: '100%',
          ...(mobile ? { minWidth: '3.5em' } : {}),
        },
        '.cm-lineNumbers .cm-gutterElement': {
          ...(mobile ? { padding: '0 6px' } : {}),
        },
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
      ...(!mobile ? [rectangularSelection(), crosshairCursor()] : []),

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
    [activeTab?.language, activeTab?.path, wrap, activeTab?.id, loadedSupports, fontSize, mobile],
  )

  return (
    <main
      className={cn(
        'flex-1 flex flex-col min-w-0 h-full bg-editor relative pb-16 lg:pb-0 @container',
        visible ? 'flex' : 'hidden',
      )}
    >
      {/* Tab Bar */}
      {!hideTabBar && (
        <TabBar
          tabs={tabs}
          activeTabId={activeTabId}
          onTabSelect={onTabSelect}
          onTabClose={onTabClose}
          onSave={onSave}
          wrap={wrap}
          onToggleWrap={onToggleWrap}
        />
      )}

      {/* Changed-on-disk banner — shown when the active tab's file was modified
          on disk (agent write / external edit) while the user had unsaved
          edits, so its content was NOT auto-refreshed. Offers a Reload that
          discards local edits and fetches the on-disk version. Uses the
          warning semantic token so it adapts to the active theme. */}
      {activeTab?.changedOnDisk && activeTab.kind !== 'settings' && (
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

      {/* CodeMirror 6 Editor, Settings Panel, or Empty State */}
      <div className="flex-1 overflow-hidden bg-editor relative">
        {tabs.some(t => t.kind === 'settings') && (
          <div className={cn("absolute inset-0 bg-background", activeTab?.kind === 'settings' ? 'block' : 'hidden')}>
            {settingsProps && <SettingsPanel {...settingsProps} />}
          </div>
        )}

        {tabs.filter(t => t.kind !== 'settings').map(tab => {
          if (tab.isBinary) {
            const ext = tab.name.split('.').pop()?.toLowerCase() || ''
            const isImage = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)
            return (
              <div key={tab.id} className={cn("absolute inset-0 flex items-center justify-center bg-editor", activeTabId === tab.id ? 'block' : 'hidden')}>
                {isImage ? (
                  <div className="flex flex-col items-center gap-3 p-6 max-h-full overflow-auto">
                    <img
                      src={`/workspaces/${tab.workspaceId ?? ''}/file?path=${encodeURIComponent(tab.path)}`}
                      alt={tab.name}
                      className="max-w-full max-h-[calc(100vh-200px)] rounded-lg border border-border shadow-lg"
                    />
                    <span className="text-xs text-muted-foreground">{tab.name}</span>
                  </div>
                ) : (
                  <div className="flex flex-col items-center gap-3 text-muted-foreground">
                    <FileX className="w-12 h-12" />
                    <p className="text-sm font-medium">Binary file — preview not available</p>
                    <p className="text-xs text-muted-foreground/70">{tab.name}</p>
                  </div>
                )}
              </div>
            )
          }
          return (
          <div key={tab.id} className={cn("absolute inset-0", activeTabId === tab.id ? 'block' : 'hidden')}>
            <CodeMirror
              value={tab.content}
              onChange={(val) => {
                if (activeTabId === tab.id) onContentChange(val)
              }}
              extensions={extensions}
              theme={oneDark}
              height="100%"
              className="h-full"
              onCreateEditor={(view) => {
                editorViewsRef.current[tab.id] = view
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
          </div>
          )
        })}

        {tabs.length === 0 && (
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
          {activeTab?.kind !== 'settings' && !activeTab?.isBinary && (
            <div className="flex items-center gap-1">
              <button
                onClick={() => setFontSize((s) => Math.max(8, s - 1))}
                className="p-0.5 hover:bg-white/10 rounded transition"
                aria-label="Decrease font size"
                title="Decrease font size"
              >
                <ZoomOut className="w-3 h-3" />
              </button>
              <span className="tabular-nums w-7 text-center">{fontSize}</span>
              <button
                onClick={() => setFontSize((s) => Math.min(32, s + 1))}
                className="p-0.5 hover:bg-white/10 rounded transition"
                aria-label="Increase font size"
                title="Increase font size"
              >
                <ZoomIn className="w-3 h-3" />
              </button>
            </div>
          )}
          <span className="hidden md:inline">{activeTab?.kind === 'settings' ? 'Settings' : (activeTab?.language || 'Plain Text')}</span>
          <span className="hidden md:inline">UTF-8</span>
          <span className="hidden md:inline">LF</span>
          <span>Ln 1, Col 1</span>
        </div>
      </div>
    </main>
  )
}
