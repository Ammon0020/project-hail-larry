import CodeMirror from '@uiw/react-codemirror'
import { javascript } from '@codemirror/lang-javascript'
import { oneDark } from '@codemirror/theme-one-dark'
import { FileCode, Circle, X, GitCompare, Save, GitBranch, CircleAlert, TriangleAlert, FileText } from 'lucide-react'
import { cn } from '@/lib/utils'
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

  /** Pick CodeMirror language extension based on Tab.language. */
  const getExtensions = (lang: string) => {
    if (['javascript', 'js', 'jsx', 'ts', 'tsx'].includes(lang)) {
      return [javascript({ jsx: lang === 'jsx' || lang === 'tsx', typescript: lang === 'ts' || lang === 'tsx' })]
    }
    // Basic setup for everything else (no specific language extension)
    return []
  }

  return (
    <main
      className={cn(
        'flex-1 flex flex-col min-w-0 h-full bg-editor relative pb-16 lg:pb-0',
        visible ? 'flex' : 'hidden',
      )}
    >
      {/* Tab Bar */}
      <div className="flex items-center bg-panel border-b border-background shrink-0 h-9">
        <div className="flex overflow-x-auto hide-scrollbar">
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
        <div className="flex-1" />
        {/* Editor actions: Diff + Save (Blueprint Sec 14 — file sync) */}
        {activeTab && (
          <div className="hidden md:flex gap-1.5 pr-3">
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
            extensions={getExtensions(activeTab.language)}
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
