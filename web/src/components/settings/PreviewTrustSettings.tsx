import { useState } from 'react'
import { ErrorNote } from './shared'
import { withAsyncState } from './utils'

export function PreviewTrustSettings({ workspaceId, workspaceTrusted, onSetWorkspaceTrust }: {
  workspaceId?: string
  workspaceTrusted?: boolean | null
  onSetWorkspaceTrust?: (workspaceId: string, trusted: boolean | null) => Promise<void>
}) {
  const [trustBusy, setTrustBusy] = useState(false)
  const [trustError, setTrustError] = useState<string | null>(null)
  async function handleSetTrust(value: boolean | null) {
    if (!workspaceId || !onSetWorkspaceTrust) return
    await withAsyncState(setTrustBusy, setTrustError, () => onSetWorkspaceTrust(workspaceId, value))
  }
  return <section id="preview" className="scroll-mt-4 space-y-6">
    {workspaceId && onSetWorkspaceTrust && <div className="space-y-3 rounded-lg border border-border bg-panel p-4">
      <div><h4 className="text-sm font-semibold text-foreground">Preview trust</h4><p className="mt-1 text-xs text-muted-foreground">Controls how HTML previews from this workspace handle cross-origin resources.</p></div>
      {trustError && <ErrorNote message={trustError} />}
      <div className="mt-1 flex flex-col gap-3">{([
        ['ask', workspaceTrusted == null, null, 'Ask on first preview', 'Prompt before rendering HTML previews from this workspace.'],
        ['trusted', workspaceTrusted === true, true, 'Trusted', 'Allow cross-origin resources (CDNs, APIs, WebSockets) in HTML previews.'],
        ['untrusted', workspaceTrusted === false, false, 'Untrusted', 'Block cross-origin resources and exfiltration channels in HTML previews.'],
      ] as const).map(([value, checked, trustValue, title, description]) => <label key={value} className="flex cursor-pointer items-start gap-2"><input type="radio" name="preview-trust" value={value} checked={checked} onChange={() => void handleSetTrust(trustValue)} disabled={trustBusy} className="mt-0.5 size-4 cursor-pointer border-input text-primary accent-primary focus:ring-primary" /><div className="space-y-0.5"><span className="block text-sm text-foreground">{title}</span><span className="block text-xs text-muted-foreground">{description}</span></div></label>)}</div>
    </div>}
    {(!workspaceId || !onSetWorkspaceTrust) && <p className="text-sm text-muted-foreground">Open a workspace to configure its preview trust policy.</p>}
  </section>
}
