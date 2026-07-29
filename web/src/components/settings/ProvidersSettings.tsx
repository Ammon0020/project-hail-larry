import { useEffect, useState } from 'react'
import { Settings } from 'lucide-react'
import type { ProviderInfo } from '@/lib/api'
import { disableProvider, listProviders, setProvider, UnsupportedProvidersError } from '@/lib/api'
import { ErrorNote } from './shared'

type ProvidersStatus = 'idle' | 'loading' | 'unsupported' | 'loaded' | 'error'

export function ProvidersSettings({ active, activeSessionId }: { active: boolean; activeSessionId?: string | null }) {
  const [providers, setProviders] = useState<ProviderInfo[]>([])
  const [providersStatus, setProvidersStatus] = useState<ProvidersStatus>('idle')
  const [providersError, setProvidersError] = useState<string | null>(null)
  const [providerBusy, setProviderBusy] = useState<string | null>(null)
  async function loadProviders(sessionId: string) {
    setProvidersStatus('loading')
    setProvidersError(null)
    try {
      const list = await listProviders(sessionId)
      setProviders(list)
      setProvidersStatus('loaded')
    } catch (error) {
      if (error instanceof UnsupportedProvidersError) {
        setProvidersStatus('unsupported')
        setProviders([])
      } else {
        setProvidersStatus('error')
        setProvidersError(error instanceof Error ? error.message : String(error))
      }
    }
  }
  useEffect(() => {
    if (!active || !activeSessionId) return
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadProviders(activeSessionId)
  }, [active, activeSessionId])
  async function handleProviderAction(providerId: string, action: 'set' | 'disable', apiType?: string, baseUrl?: string, headersText?: string) {
    if (!activeSessionId) return
    setProviderBusy(providerId)
    setProvidersError(null)
    try {
      if (action === 'set' && apiType && baseUrl) await setProvider(activeSessionId, providerId, apiType, baseUrl, parseHeaders(headersText ?? ''))
      else await disableProvider(activeSessionId, providerId)
      await loadProviders(activeSessionId)
    } catch (error) {
      setProvidersError(error instanceof Error ? error.message : String(error))
    } finally { setProviderBusy(null) }
  }
  return <section id="providers" className="scroll-mt-4 space-y-6">
    <div className="flex items-center gap-2"><Settings className="w-4 h-4 text-muted-foreground" /><h3 className="text-base font-semibold text-foreground">Providers (advanced)</h3></div>
    <div className="p-4 bg-panel border border-border rounded-lg space-y-3">
      <div className="flex items-center gap-2"><h4 className="font-semibold text-sm text-foreground">Providers (advanced)</h4>{providersStatus === 'loading' && <span className="text-xs text-muted-foreground">Loading…</span>}</div>
      <p className="text-xs text-muted-foreground">Configure the model providers the active session's agent can use. Per-session; advanced.</p>
      {!activeSessionId && <p className="text-xs text-muted-foreground italic">Open a chat session to configure providers.</p>}
      {activeSessionId && providersStatus === 'unsupported' && <p className="text-xs text-muted-foreground italic">The current agent does not support runtime provider configuration.</p>}
      {activeSessionId && providersStatus === 'error' && providersError && <ErrorNote message={providersError} mono className="p-3" />}
      {activeSessionId && providersStatus === 'loaded' && providers.length === 0 && <p className="text-xs text-muted-foreground italic">No configurable providers for this agent.</p>}
      {activeSessionId && (providersStatus === 'loaded' || providersStatus === 'loading') && providers.length > 0 && <div className="space-y-3">{providers.map(provider => <ProviderRow key={`${provider.id}:${provider.current?.apiType ?? ''}:${provider.current?.baseUrl ?? ''}`} provider={provider} busy={providerBusy === provider.id} error={providerBusy === provider.id ? providersError : null} onSet={(apiType, baseUrl, headersText) => handleProviderAction(provider.id, 'set', apiType, baseUrl, headersText)} onDisable={() => handleProviderAction(provider.id, 'disable')} />)}</div>}
    </div>
  </section>
}

function parseHeaders(text: string): Record<string, string> {
  const headers: Record<string, string> = {}
  for (const raw of text.split('\n')) {
    const line = raw.trim()
    const index = line.indexOf(':')
    if (!line || index <= 0) continue
    const key = line.slice(0, index).trim()
    if (key) headers[key] = line.slice(index + 1).trim()
  }
  return headers
}

function ProviderRow({ provider, busy, error, onSet, onDisable }: { provider: ProviderInfo; busy: boolean; error: string | null; onSet: (apiType: string, baseUrl: string, headersText: string) => void; onDisable: () => void }) {
  const supported = provider.supported
  const [apiType, setApiType] = useState(provider.current?.apiType ?? supported[0] ?? '')
  const [baseUrl, setBaseUrl] = useState(provider.current?.baseUrl ?? '')
  const [headersText, setHeadersText] = useState('')
  const canSet = !busy && apiType && baseUrl.trim().length > 0
  const canDisable = !busy && !provider.required && !!provider.current
  return <div className="p-3 bg-background border border-border rounded-md space-y-2">
    <div className="flex items-center gap-2 flex-wrap"><span className="font-mono text-sm text-foreground">{provider.id}</span>{provider.required && <span className="text-[10px] px-1.5 py-0.5 rounded border border-amber-500/30 bg-amber-500/10 text-amber-600 dark:text-amber-400">required</span>}{supported.length > 0 && <span className="text-[10px] text-muted-foreground">supports: {supported.join(', ')}</span>}</div>
    <div className="text-xs text-muted-foreground">{provider.current ? <>current: <span className="font-mono">{provider.current.apiType}</span> · <span className="font-mono break-all">{provider.current.baseUrl}</span></> : <span className="italic">disabled</span>}</div>
    <div className="grid grid-cols-1 md:grid-cols-[120px_1fr] gap-2"><select value={apiType} onChange={event => setApiType(event.target.value)} disabled={busy || supported.length === 0} aria-label={`${provider.id} api type`} className="bg-background border border-input rounded-md px-2 py-1.5 text-sm disabled:opacity-50">{supported.length === 0 && <option value="">—</option>}{supported.map(type => <option key={type} value={type}>{type}</option>)}</select><input type="text" value={baseUrl} onChange={event => setBaseUrl(event.target.value)} placeholder="https://api.example.com" disabled={busy} aria-label={`${provider.id} base URL`} className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm disabled:opacity-50" /></div>
    <textarea value={headersText} onChange={event => setHeadersText(event.target.value)} rows={2} disabled={busy} placeholder={'Optional headers (one per line):\nAuthorization: Bearer ...'} aria-label={`${provider.id} headers`} className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-xs font-mono disabled:opacity-50" />
    {error && <ErrorNote message={error} mono />}
    <div className="flex items-center gap-2"><button type="button" onClick={() => onSet(apiType, baseUrl.trim(), headersText)} disabled={!canSet} className="px-3 py-1.5 text-xs font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition disabled:opacity-50">{busy ? 'Saving…' : 'Set'}</button>{provider.required ? <span className="text-[10px] text-muted-foreground">Required providers cannot be disabled.</span> : <button type="button" onClick={onDisable} disabled={!canDisable} className="px-3 py-1.5 text-xs font-medium text-foreground bg-secondary hover:bg-accent rounded-md border border-input transition disabled:opacity-50">Disable</button>}</div>
  </div>
}
