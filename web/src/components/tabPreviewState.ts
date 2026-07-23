import type { Tab } from '@/types'

/**
 * Shared Preview-button state for the desktop header TabBar and mobile
 * EditorPane TabBar (same active tab, two hosts). HTML/HTM → browse preview;
 * other previewable text → FileViewer toggle (pressed when viewMode=preview).
 */
export function editorTabPreviewState(tab: Tab | null | undefined): {
  show: boolean
  active: boolean
  isHtmlEntry: boolean
} {
  const eligible =
    !!tab && tab.kind !== 'settings' && tab.kind !== 'preview'
  const isHtmlEntry = eligible && /\.html?$/i.test(tab.path)
  const show =
    eligible && !tab.isBinary && (!!tab.previewable || isHtmlEntry)
  const active = !isHtmlEntry && tab?.viewMode === 'preview'
  return { show, active, isHtmlEntry }
}
