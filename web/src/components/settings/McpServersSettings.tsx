import { useEffect, useMemo, useState } from 'react'
import CodeMirror from '@uiw/react-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import { Check, ChevronDown, ChevronRight, HelpCircle, RotateCcw, Save } from 'lucide-react'
import type { McpServerConfig } from '@/types'
import { getMcpConfig, patchMcpServer, putMcpConfig } from '@/lib/api'
import { cn } from '@/lib/utils'
import { CopyableExample, ErrorNote } from './shared'
import { withAsyncState } from './utils'

const STDIO_EXAMPLE = `{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/Users/me/projects"],
      "env": { "NODE_ENV": "production" }
    }
  }
}`
const HTTP_EXAMPLE = `{
  "mcpServers": {
    "remote-api": {
      "type": "http",
      "url": "https://example.com/mcp",
      "headers": { "Authorization": "Bearer \${API_TOKEN}" }
    }
  }
}`

export function McpServersSettings() {
  const [mcpText, setMcpText] = useState('')
  const [mcpOriginal, setMcpOriginal] = useState('')
  const [mcpLoading, setMcpLoading] = useState(true)
  const [mcpSaving, setMcpSaving] = useState(false)
  const [mcpError, setMcpError] = useState<string | null>(null)
  const [mcpSaved, setMcpSaved] = useState(false)
  const [togglingServer, setTogglingServer] = useState<string | null>(null)
  const [showQuickRef, setShowQuickRef] = useState(false)
  async function loadMcp() {
    const text = await withAsyncState(setMcpLoading, setMcpError, getMcpConfig)
    if (!text) return
    setMcpText(text)
    setMcpOriginal(text)
  }
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadMcp()
  }, [])
  const servers = useMemo(() => {
    try {
      const parsed = JSON.parse(mcpOriginal)
      return Object.entries(parsed.mcpServers || {}).map(([name, config]) => ({ name, enabled: (config as McpServerConfig).enabled !== false }))
    } catch { return [] }
  }, [mcpOriginal])
  const disabledCount = servers.filter(server => !server.enabled).length
  async function handleSave() {
    const ok = await withAsyncState(setMcpSaving, setMcpError, () => putMcpConfig(mcpText))
    if (ok === undefined) return
    setMcpOriginal(mcpText)
    setMcpSaved(true)
    setTimeout(() => setMcpSaved(false), 2000)
    // Notify other settings sections (e.g. ProfilesSettings) that the MCP
    // server list changed so they can refetch without reopening Settings.
    window.dispatchEvent(new CustomEvent('mcp-changed'))
  }
  function handleRevert() { setMcpError(null); setMcpText(mcpOriginal) }
  async function handleToggle(name: string, enabled: boolean) {
    if (mcpText !== mcpOriginal && !window.confirm('Discard unsaved editor changes to toggle this server?')) return
    setTogglingServer(name)
    try { await patchMcpServer(name, enabled); await loadMcp(); window.dispatchEvent(new CustomEvent('mcp-changed')) } catch (error: unknown) { setMcpError(error instanceof Error ? error.message : String(error)) } finally { setTogglingServer(null) }
  }
  return <section id="mcp-servers" className="flex scroll-mt-4 flex-col gap-4">
    <div className="flex shrink-0 items-center justify-between"><h3 className="text-base font-semibold text-foreground">MCP Servers</h3><button type="button" title="See docs/reference/mcp/" aria-label="MCP documentation" className="rounded-md p-1 text-muted-foreground transition hover:text-foreground"><HelpCircle className="size-4" /></button></div>
    <div className="flex shrink-0 flex-wrap gap-2">{servers.length === 0 && !mcpLoading && <span className="text-xs text-muted-foreground">No servers configured.</span>}{servers.map(server => <button key={server.name} onClick={() => handleToggle(server.name, !server.enabled)} disabled={togglingServer === server.name} title={server.enabled ? 'Disable' : 'Enable'} className={cn('flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs transition', server.enabled ? 'border-primary/30 bg-primary/10 text-primary' : 'border-border bg-muted text-muted-foreground', togglingServer === server.name && 'opacity-50')}><span className="font-mono">{server.name}</span>{togglingServer === server.name ? <span className="inline-block size-3" /> : server.enabled ? <Check className="size-3" /> : <span className="size-3 text-center leading-none">×</span>}</button>)}</div>
    <div className="shrink-0 text-xs text-muted-foreground">{servers.length} server{servers.length === 1 ? '' : 's'}{disabledCount > 0 && `, ${disabledCount} disabled`}</div>
    {mcpError && <ErrorNote message={mcpError} mono className="shrink-0 p-3" />}
    <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border"><CodeMirror value={mcpText} onChange={setMcpText} extensions={[json()]} theme={oneDark} height="100%" className="h-full text-[13px]" basicSetup={{ lineNumbers: true, foldGutter: true, highlightActiveLine: true, bracketMatching: true, closeBrackets: true, indentOnInput: true }} /></div>
    <div className="flex shrink-0 items-center gap-2"><button onClick={handleSave} disabled={mcpSaving || mcpText === mcpOriginal} className="flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition hover:bg-primary/90 disabled:opacity-50"><Save className="size-3.5" />{mcpSaving ? 'Saving...' : 'Save'}</button><button onClick={handleRevert} disabled={mcpText === mcpOriginal} className="flex items-center gap-2 rounded-md border border-input bg-secondary px-3 py-1.5 text-xs font-medium text-foreground transition hover:bg-accent disabled:opacity-50"><RotateCcw className="size-3.5" />Revert</button>{mcpSaved && <span className="flex items-center gap-1 text-xs text-green-500"><Check className="size-3.5" />Saved</span>}</div>
    <div className="shrink-0 rounded-md border border-border"><button onClick={() => setShowQuickRef(!showQuickRef)} className="flex w-full items-center gap-2 rounded-md bg-panel px-3 py-2 text-xs font-medium text-foreground transition hover:bg-accent">{showQuickRef ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}Quick reference</button>{showQuickRef && <div className="space-y-3 border-t border-border p-3"><CopyableExample label="stdio" text={STDIO_EXAMPLE} /><CopyableExample label="http" text={HTTP_EXAMPLE} /><p className="text-xs text-muted-foreground">Environment variables use <code className="rounded bg-muted px-1 font-mono">{'${VAR}'}</code> syntax and are expanded by the backend.</p><p className="text-xs text-muted-foreground">Compatible with Claude Desktop, Cursor, and Windsurf config files — paste directly into the editor above.</p></div>}</div>
  </section>
}
