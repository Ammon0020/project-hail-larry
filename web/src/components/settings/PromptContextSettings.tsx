import { useEffect, useState } from 'react'
import { Settings } from 'lucide-react'
import type { PromptContextSettings as PromptContextSettingsState } from '@/types'
import { getPromptContextSettings, putPromptContextSettings } from '@/lib/api'
import { ErrorNote, LabeledInput } from './shared'
import { withAsyncState } from './utils'

export function PromptContextSettings() {
  const [promptContext, setPromptContext] = useState<PromptContextSettingsState | null>(null)
  const [promptContextOriginal, setPromptContextOriginal] = useState<PromptContextSettingsState | null>(null)
  const [promptContextLoading, setPromptContextLoading] = useState(true)
  const [promptContextSaving, setPromptContextSaving] = useState(false)
  const [promptContextError, setPromptContextError] = useState<string | null>(null)
  async function loadPromptContext() {
    const settings = await withAsyncState(setPromptContextLoading, setPromptContextError, getPromptContextSettings)
    if (!settings) return
    setPromptContext(settings)
    setPromptContextOriginal(settings)
  }
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadPromptContext()
  }, [])
  async function savePromptContext() {
    if (!promptContext) return
    const settings = await withAsyncState(setPromptContextSaving, setPromptContextError, () => putPromptContextSettings(promptContext))
    if (!settings) return
    setPromptContext(settings)
    setPromptContextOriginal(settings)
  }
  function updatePromptContext(key: keyof PromptContextSettingsState, value: string) {
    const number = Number(value)
    if (!Number.isInteger(number) || number < 0 || number > 100) return
    setPromptContext(current => current ? { ...current, [key]: number } : current)
  }
  return <section id="prompt-context" className="scroll-mt-4 space-y-6">
    <div className="flex items-center gap-2"><Settings className="w-4 h-4 text-muted-foreground" /><h3 className="text-base font-semibold text-foreground">Prompt Context</h3></div>
    <div className="p-4 bg-panel border border-border rounded-lg space-y-3">
      <div><h4 className="font-semibold text-sm text-foreground">Prompt context</h4><p className="mt-1 text-xs text-muted-foreground">Only relative paths are added automatically. File contents are never added from open tabs; explicit editor selections remain separate context.</p></div>
      {promptContextError && <ErrorNote message={promptContextError} />}
      {promptContextLoading || !promptContext ? <p className="text-xs text-muted-foreground">Loading context settings…</p> : <>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <LabeledInput label="Open and recently edited paths" type="number" min={0} max={100} wrapperClass="space-y-1" labelClass="block text-xs text-foreground" value={promptContext.openFileLimit} onChange={value => updatePromptContext('openFileLimit', value)} />
          <LabeledInput label="Top-level workspace entries" type="number" min={0} max={100} wrapperClass="space-y-1" labelClass="block text-xs text-foreground" value={promptContext.workspaceFileListLimit} onChange={value => updatePromptContext('workspaceFileListLimit', value)} />
        </div>
        <div className="flex items-center gap-2"><button type="button" onClick={savePromptContext} disabled={promptContextSaving || JSON.stringify(promptContext) === JSON.stringify(promptContextOriginal)} className="px-3 py-1.5 text-xs font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition disabled:opacity-50">{promptContextSaving ? 'Saving…' : 'Save context limits'}</button><span className="text-[11px] text-muted-foreground">0 disables a list; maximum 100.</span></div>
      </>}
    </div>
  </section>
}
