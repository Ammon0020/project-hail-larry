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
  return <section id="mcp-servers" className="scroll-mt-4 flex flex-col gap-4">
    <div className="flex items-center justify-between shrink-0"><h3 className="text-base font-semibold text-foreground">MCP Servers</h3><button type="button" title="See docs/reference/mcp/" aria-label="MCP documentation" className="p-1 text-muted-foreground hover:text-foreground rounded-md transition"><HelpCircle className="w-4 h-4" /></button></div>
    <div className="flex flex-wrap gap-2 shrink-0">{servers.length === 0 && !mcpLoading && <span className="text-xs text-muted-foreground">No servers configured.</span>}{servers.map(server => <button key={server.name} onClick={() => handleToggle(server.name, !server.enabled)} disabled={togglingServer === server.name} title={server.enabled ? 'Disable' : 'Enable'} className={cn('flex items-center gap-1.5 px-2.5 py-1 text-xs rounded-full border transition', server.enabled ? 'bg-primary/10 border-primary/30 text-primary' : 'bg-muted border-border text-muted-foreground', togglingServer === server.name && 'opacity-50')}><span className="font-mono">{server.name}</span>{togglingServer === server.name ? <span className="w-3 h-3 inline-block" /> : server.enabled ? <Check className="w-3 h-3" /> : <span className="w-3 h-3 leading-none text-center">×</span>}</button>)}</div>
    <div className="text-xs text-muted-foreground shrink-0">{servers.length} server{servers.length === 1 ? '' : 's'}{disabledCount > 0 && `, ${disabledCount} disabled`}</div>
    {mcpError && <ErrorNote message={mcpError} mono className="p-3 shrink-0" />}
    <div className="flex-1 min-h-0 overflow-hidden border border-border rounded-md"><CodeMirror value={mcpText} onChange={setMcpText} extensions={[json()]} theme={oneDark} height="100%" className="h-full text-[13px]" basicSetup={{ lineNumbers: true, foldGutter: true, highlightActiveLine: true, bracketMatching: true, closeBrackets: true, indentOnInput: true }} /></div>
    <div className="flex items-center gap-2 shrink-0"><button onClick={handleSave} disabled={mcpSaving || mcpText === mcpOriginal} className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition disabled:opacity-50"><Save className="w-3.5 h-3.5" />{mcpSaving ? 'Saving...' : 'Save'}</button><button onClick={handleRevert} disabled={mcpText === mcpOriginal} className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-foreground bg-secondary hover:bg-accent rounded-md border border-input transition disabled:opacity-50"><RotateCcw className="w-3.5 h-3.5" />Revert</button>{mcpSaved && <span className="flex items-center gap-1 text-xs text-green-500"><Check className="w-3.5 h-3.5" />Saved</span>}</div>
    <div className="shrink-0 border border-border rounded-md"><button onClick={() => setShowQuickRef(!showQuickRef)} className="flex items-center gap-2 w-full px-3 py-2 text-xs font-medium text-foreground bg-panel hover:bg-accent rounded-md transition">{showQuickRef ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronRight className="w-3.5 h-3.5" />}Quick reference</button>{showQuickRef && <div className="p-3 space-y-3 border-t border-border"><CopyableExample label="stdio" text={STDIO_EXAMPLE} /><CopyableExample label="http" text={HTTP_EXAMPLE} /><p className="text-xs text-muted-foreground">Environment variables use <code className="font-mono bg-muted px-1 rounded">{'${VAR}'}</code> syntax and are expanded by the backend.</p><p className="text-xs text-muted-foreground">Compatible with Claude Desktop, Cursor, and Windsurf config files — paste directly into the editor above.</p></div>}</div>
  </section>
}
