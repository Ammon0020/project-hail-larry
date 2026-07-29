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
    {workspaceId && onSetWorkspaceTrust && <div className="p-4 bg-panel border border-border rounded-lg space-y-3">
      <div><h4 className="font-semibold text-sm text-foreground">Preview trust</h4><p className="mt-1 text-xs text-muted-foreground">Controls how HTML previews from this workspace handle cross-origin resources.</p></div>
      {trustError && <ErrorNote message={trustError} />}
      <div className="flex flex-col gap-3 mt-1">{([
        ['ask', workspaceTrusted == null, null, 'Ask on first preview', 'Prompt before rendering HTML previews from this workspace.'],
        ['trusted', workspaceTrusted === true, true, 'Trusted', 'Allow cross-origin resources (CDNs, APIs, WebSockets) in HTML previews.'],
        ['untrusted', workspaceTrusted === false, false, 'Untrusted', 'Block cross-origin resources and exfiltration channels in HTML previews.'],
      ] as const).map(([value, checked, trustValue, title, description]) => <label key={value} className="flex items-start gap-2 cursor-pointer"><input type="radio" name="preview-trust" value={value} checked={checked} onChange={() => void handleSetTrust(trustValue)} disabled={trustBusy} className="text-primary focus:ring-primary h-4 w-4 border-input accent-primary cursor-pointer mt-0.5" /><div className="space-y-0.5"><span className="block text-sm text-foreground">{title}</span><span className="block text-xs text-muted-foreground">{description}</span></div></label>)}</div>
    </div>}
    {(!workspaceId || !onSetWorkspaceTrust) && <p className="text-sm text-muted-foreground">Open a workspace to configure its preview trust policy.</p>}
  </section>
}
