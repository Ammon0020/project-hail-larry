import { useState } from 'react'
import CodeMirror from '@uiw/react-codemirror'
import { javascript } from '@codemirror/lang-javascript'
import { oneDark } from '@codemirror/theme-one-dark'
import { FileCode, Circle, X, GitCompare, Save, GitBranch, CircleAlert, TriangleAlert } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * Editor pane — tabbed CodeMirror 6 with diff/save and status bar
 * (Blueprint Sec 17 — center editor).
 *
 * In production, file content arrives via FileRevisionUpdated events.
 * Saves use expectedRevision for three-way merge conflict detection (Sec 14).
 */
export function EditorPane({
  content,
  visible,
}: {
  content: string
  visible: boolean
}) {
  const [code, setCode] = useState(content)

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
          {/* Active tab */}
          <div className="flex items-center gap-2 px-3 h-9 bg-editor text-sm text-gray-200 border-t-2 border-blue-500 shrink-0 border-r border-background">
            <FileCode className="w-3.5 h-3.5 text-yellow-400" /> server.js
            <Circle className="w-2 h-2 text-blue-400 fill-blue-400" />
            <X className="w-3.5 h-3.5 text-gray-500 hover:text-white cursor-pointer ml-1" />
          </div>
          {/* Inactive tab */}
          <div className="hidden md:flex items-center gap-2 px-3 h-9 bg-panel text-sm text-gray-500 border-r border-background shrink-0 hover:bg-editor/50 transition">
            <FileCode className="w-3.5 h-3.5 text-yellow-400" /> routes/index.js
            <X className="w-3.5 h-3.5 text-gray-600 hover:text-gray-300 cursor-pointer ml-1" />
          </div>
        </div>
        <div className="flex-1" />
        {/* Editor actions: Diff + Save (Blueprint Sec 14 — file sync) */}
        <div className="hidden md:flex gap-1.5 pr-3">
          <button className="text-xs font-semibold bg-gray-800 hover:bg-gray-700 text-gray-300 px-2.5 py-1 rounded transition flex items-center gap-1.5">
            <GitCompare className="w-3 h-3" /> Diff
          </button>
          <button className="text-xs font-semibold bg-blue-600 hover:bg-blue-500 text-white px-2.5 py-1 rounded flex items-center gap-1.5 transition">
            <Save className="w-3 h-3" /> Save
          </button>
        </div>
      </div>

      {/* CodeMirror 6 Editor (Blueprint Sec 17 — CodeMirror 6) */}
      <div className="flex-1 overflow-auto bg-editor">
        <CodeMirror
          value={code}
          onChange={setCode}
          extensions={[javascript({ jsx: false })]}
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
      </div>

      {/* Status Bar (Blueprint Sec 17) */}
      <div className="flex items-center justify-between bg-status-bar text-white text-[10px] md:text-[11px] px-3 py-0.5 shrink-0">
        <div className="flex items-center gap-3">
          <span className="flex items-center gap-1"><GitBranch className="w-3 h-3" /> main</span>
          <span className="hidden md:flex items-center gap-1"><CircleAlert className="w-3 h-3" /> 0 errors</span>
          <span className="hidden md:flex items-center gap-1"><TriangleAlert className="w-3 h-3" /> 0 warnings</span>
        </div>
        <div className="flex items-center gap-3">
          <span className="hidden md:inline">JavaScript</span>
          <span className="hidden md:inline">UTF-8</span>
          <span className="hidden md:inline">LF</span>
          <span>Ln 21, Col 3</span>
        </div>
      </div>
    </main>
  )
}
