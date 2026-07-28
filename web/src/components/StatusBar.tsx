import { GitBranch, ZoomOut, ZoomIn } from 'lucide-react'
import type { Tab } from '@/types'

/**
 * Status Bar (Blueprint Sec 17) — bottom-of-app strip showing git branch,
 * font-size controls, language, and the cursor position. On desktop it spans
 * the full app width (rendered by App.tsx after the main shell); on mobile it
 * is rendered inside EditorPane so it sits above the bottom nav and spans only
 * the editor pane. Responsive visibility of individual segments is handled via
 * `md:` breakpoints. Diagnostics / encoding / line-endings are intentionally
 * omitted: no source tracks them, and showing hardcoded values would lie.
 */
export function StatusBar({
  activeTab,
  fontSize,
  onFontSizeChange,
  gitBranch,
  cursorPos,
}: {
  activeTab: Tab | null
  fontSize: number
  onFontSizeChange: (fn: (s: number) => number) => void
  gitBranch?: string | null
  cursorPos?: { line: number; col: number }
}) {
  const isCodeTab =
    activeTab?.kind !== 'settings' && activeTab?.kind !== 'preview' && !activeTab?.isBinary
  return (
    <div className="flex items-center justify-between bg-status-bar text-white text-[10px] md:text-[11px] px-3 py-0.5 shrink-0">
      <div className="flex items-center gap-3">
        {gitBranch && (
          <span className="flex items-center gap-1"><GitBranch className="w-3 h-3" /> {gitBranch}</span>
        )}
      </div>
      <div className="flex items-center gap-3">
        {activeTab?.kind !== 'settings' && activeTab?.kind !== 'preview' && !activeTab?.isBinary && (
          <div className="flex items-center gap-1">
            <button
              onClick={() => onFontSizeChange((s) => Math.max(8, s - 1))}
              className="p-0.5 hover:bg-white/10 rounded transition"
              aria-label="Decrease font size"
              title="Decrease font size"
            >
              <ZoomOut className="w-3 h-3" />
            </button>
            <span className="tabular-nums w-7 text-center">{fontSize}</span>
            <button
              onClick={() => onFontSizeChange((s) => Math.min(32, s + 1))}
              className="p-0.5 hover:bg-white/10 rounded transition"
              aria-label="Increase font size"
              title="Increase font size"
            >
              <ZoomIn className="w-3 h-3" />
            </button>
          </div>
        )}
        <span className="hidden md:inline">
          {activeTab?.kind === 'settings'
            ? 'Settings'
            : activeTab?.kind === 'preview'
              ? 'Preview'
              : (activeTab?.language || 'Plain Text')}
        </span>
        {isCodeTab && cursorPos && (
          <span className="tabular-nums">Ln {cursorPos.line}, Col {cursorPos.col}</span>
        )}
      </div>
    </div>
  )
}
