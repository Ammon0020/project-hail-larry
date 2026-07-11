import { useState, useEffect, useMemo } from 'react'
import CodeMirror from '@uiw/react-codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import {
  Search,
  Plus,
  Trash2,
  AlertTriangle,
  Monitor,
  Settings,
  Save,
  RotateCcw,
  Copy,
  Check,
  ChevronDown,
  ChevronRight,
  HelpCircle,
} from 'lucide-react'
import type { AgentInfo } from '@/lib/api'
import { getMcpConfig, putMcpConfig, patchMcpServer } from '@/lib/api'
import { cn } from '@/lib/utils'

type McpServerConfig = {
  enabled?: boolean
  command?: string
  args?: string[]
  env?: Record<string, string>
  type?: string
  url?: string
  headers?: Record<string, string>
  [key: string]: unknown
}

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

export function SettingsPanel({
  agents,
  onAddAgent,
  onDeleteAgent,
  onAutodetect,
}: {
  agents: AgentInfo[]
  onAddAgent: (a: AgentInfo) => Promise<void>
  onDeleteAgent: (id: string) => Promise<void>
  onAutodetect: () => Promise<AgentInfo[]>
}) {
  const [activeTab, setActiveTab] = useState<'agents' | 'mcp' | 'general'>('agents')
  const [isDetecting, setIsDetecting] = useState(false)

  // New agent form state
  const [showAddForm, setShowAddForm] = useState(false)
  const [newAgent, setNewAgent] = useState<Partial<AgentInfo>>({ models: [] })
  const [newModel, setNewModel] = useState({ id: '', name: '' })

  // MCP tab state
  const [mcpText, setMcpText] = useState('')
  const [mcpOriginal, setMcpOriginal] = useState('')
  const [mcpLoading, setMcpLoading] = useState(true)
  const [mcpSaving, setMcpSaving] = useState(false)
  const [mcpError, setMcpError] = useState<string | null>(null)
  const [mcpSaved, setMcpSaved] = useState(false)
  const [togglingServer, setTogglingServer] = useState<string | null>(null)
  const [showQuickRef, setShowQuickRef] = useState(false)

  const handleAutodetect = async () => {
    setIsDetecting(true)
    try {
      const detected = await onAutodetect()
      for (const d of detected) {
        const existing = agents.find(a => a.id === d.id)
        if (!existing) {
          await onAddAgent(d)
        } else {
          await onAddAgent({ ...existing, models: d.models, command: d.command })
        }
      }
    } catch (err) {
      console.error(err)
    } finally {
      setIsDetecting(false)
    }
  }

  const handleAddAgent = async () => {
    if (!newAgent.id || !newAgent.name || !newAgent.command) return
    await onAddAgent(newAgent as AgentInfo)
    setShowAddForm(false)
    setNewAgent({ models: [] })
  }

  const handleAddModel = () => {
    if (!newModel.id || !newModel.name) return
    setNewAgent(prev => ({
      ...prev,
      models: [...(prev.models || []), { ...newModel }],
    }))
    setNewModel({ id: '', name: '' })
  }

  useEffect(() => {
    loadMcp()
  }, [])

  async function loadMcp() {
    setMcpLoading(true)
    try {
      const text = await getMcpConfig()
      setMcpText(text)
      setMcpOriginal(text)
      setMcpError(null)
    } catch (e) {
      setMcpError(String(e))
    } finally {
      setMcpLoading(false)
    }
  }

  const servers = useMemo(() => {
    try {
      const parsed = JSON.parse(mcpOriginal)
      const entries = Object.entries(parsed.mcpServers || {})
      return entries.map(([name, cfg]) => ({
        name,
        enabled: (cfg as McpServerConfig).enabled !== false,
      }))
    } catch {
      return []
    }
  }, [mcpOriginal])

  const disabledCount = servers.filter(s => !s.enabled).length

  async function handleSave() {
    setMcpSaving(true)
    setMcpError(null)
    try {
      await putMcpConfig(mcpText)
      setMcpOriginal(mcpText)
      setMcpSaved(true)
      setTimeout(() => setMcpSaved(false), 2000)
    } catch (e: unknown) {
      setMcpError(e instanceof Error ? e.message : String(e))
    } finally {
      setMcpSaving(false)
    }
  }

  async function handleRevert() {
    setMcpError(null)
    setMcpText(mcpOriginal)
  }

  async function handleToggle(name: string, enabled: boolean) {
    setTogglingServer(name)
    try {
      await patchMcpServer(name, enabled)
      await loadMcp()
    } catch (e: unknown) {
      setMcpError(e instanceof Error ? e.message : String(e))
    } finally {
      setTogglingServer(null)
    }
  }

  const tabButtonClass = (active: boolean) =>
    cn(
      'px-4 py-2 text-sm text-left transition',
      active
        ? 'text-primary bg-primary/10 font-medium border-r-2 border-primary'
        : 'text-muted-foreground hover:text-foreground',
    )

  return (
    <div className="h-full flex">
      {/* Sidebar */}
      <div className="w-48 bg-activity-bar border-r border-border flex flex-col py-2 shrink-0">
        <button
          onClick={() => setActiveTab('agents')}
          className={tabButtonClass(activeTab === 'agents')}
        >
          Agents & Models
        </button>
        <button
          onClick={() => setActiveTab('mcp')}
          className={tabButtonClass(activeTab === 'mcp')}
        >
          MCP Servers
        </button>
        <button
          onClick={() => setActiveTab('general')}
          className={tabButtonClass(activeTab === 'general')}
        >
          General
        </button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-5 bg-background">
        {activeTab === 'agents' && (
          <div className="space-y-6">
            <div className="flex items-center justify-between">
              <h3 className="text-base font-semibold text-foreground">Configured Agents</h3>
              <div className="flex gap-2">
                <button
                  onClick={handleAutodetect}
                  disabled={isDetecting}
                  className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-foreground bg-secondary hover:bg-accent rounded-md border border-input transition disabled:opacity-50"
                >
                  <Search className="w-3.5 h-3.5" />
                  {isDetecting ? 'Detecting...' : 'Auto-detect'}
                </button>
                <button
                  onClick={() => setShowAddForm(!showAddForm)}
                  className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition"
                >
                  <Plus className="w-3.5 h-3.5" />
                  Add Custom
                </button>
              </div>
            </div>

            {showAddForm && (
              <div className="p-4 bg-panel border border-primary/30 rounded-lg space-y-4">
                <div className="grid grid-cols-2 gap-4">
                  <div>
                    <label htmlFor="agent-id-input" className="block text-xs text-muted-foreground mb-1">ID (e.g., custom-agent)</label>
                    <input
                      id="agent-id-input"
                      type="text"
                      value={newAgent.id || ''}
                      onChange={e => setNewAgent({ ...newAgent, id: e.target.value })}
                      className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm"
                    />
                  </div>
                  <div>
                    <label htmlFor="agent-name-input" className="block text-xs text-muted-foreground mb-1">Name (e.g., Custom CLI)</label>
                    <input
                      id="agent-name-input"
                      type="text"
                      value={newAgent.name || ''}
                      onChange={e => setNewAgent({ ...newAgent, name: e.target.value })}
                      className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm"
                    />
                  </div>
                  <div className="col-span-2">
                    <label htmlFor="agent-command-input" className="block text-xs text-muted-foreground mb-1">Command executable</label>
                    <input
                      id="agent-command-input"
                      type="text"
                      value={newAgent.command || ''}
                      onChange={e => setNewAgent({ ...newAgent, command: e.target.value })}
                      placeholder="e.g. claude, codex, or /absolute/path/to/bin"
                      className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm"
                    />
                  </div>
                </div>

                <div className="border-t border-border pt-3">
                  <label htmlFor="model-id-input" className="block text-xs text-muted-foreground mb-2">Models</label>
                  <div className="space-y-2 mb-2">
                    {newAgent.models?.map((m, i) => (
                      <div key={i} className="flex items-center gap-2 text-xs bg-background p-2 rounded border border-border">
                        <Monitor className="w-3 h-3 text-muted-foreground" />
                        <span className="font-mono text-primary">{m.id}</span>
                        <span className="text-muted-foreground">({m.name})</span>
                      </div>
                    ))}
                  </div>
                  <div className="flex gap-2">
                    <input
                      id="model-id-input"
                      type="text"
                      placeholder="Model ID"
                      value={newModel.id}
                      onChange={e => setNewModel({ ...newModel, id: e.target.value })}
                      className="flex-1 bg-background border border-input rounded-md px-3 py-1.5 text-sm"
                    />
                    <label htmlFor="model-name-input" className="sr-only">Model name</label>
                    <input
                      id="model-name-input"
                      type="text"
                      placeholder="Model Name"
                      value={newModel.name}
                      onChange={e => setNewModel({ ...newModel, name: e.target.value })}
                      className="flex-1 bg-background border border-input rounded-md px-3 py-1.5 text-sm"
                    />
                    <button onClick={handleAddModel} className="px-3 py-1.5 bg-secondary hover:bg-accent rounded-md text-xs">Add Model</button>
                  </div>
                </div>

                <div className="flex justify-end pt-2">
                  <button onClick={handleAddAgent} className="px-4 py-2 bg-primary hover:bg-primary/90 text-primary-foreground text-sm rounded-md font-medium">Save Agent</button>
                </div>
              </div>
            )}

            <div className="space-y-3">
              {agents.map(agent => (
                <div key={agent.id} className="p-4 bg-panel border border-border rounded-lg flex flex-col gap-3 group">
                  <div className="flex items-start justify-between">
                    <div>
                      <h4 className="font-semibold text-foreground">{agent.name}</h4>
                      <div className="flex items-center gap-2 mt-1">
                        <span className="text-xs font-mono text-muted-foreground bg-muted px-1.5 py-0.5 rounded border border-border">{agent.command}</span>
                        {agent.warning && (
                          <span className="flex items-center gap-1 text-[10px] text-amber-500 bg-amber-500/10 px-1.5 py-0.5 rounded border border-amber-500/20">
                            <AlertTriangle className="w-3 h-3" />
                            {agent.warning}
                          </span>
                        )}
                      </div>
                    </div>
                    <button
                      onClick={() => onDeleteAgent(agent.id)}
                      className="p-1.5 text-muted-foreground hover:text-destructive hover:bg-destructive/10 rounded-md transition opacity-0 group-hover:opacity-100"
                      title="Delete Agent"
                      aria-label={`Delete agent ${agent.name}`}
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>

                  <div className="flex flex-wrap gap-2">
                    {agent.models.map(m => (
                      <span key={m.id} className="text-xs px-2 py-1 bg-background border border-border text-muted-foreground rounded-md">
                        {m.name}
                      </span>
                    ))}
                  </div>
                </div>
              ))}
              {agents.length === 0 && (
                <div className="text-center p-8 border border-dashed border-input rounded-lg text-muted-foreground text-sm">
                  No agents configured. Click Auto-detect to search for installed agents.
                </div>
              )}
            </div>
          </div>
        )}

        {activeTab === 'mcp' && (
          <div className="flex flex-col h-full gap-4">
            {/* Header row */}
            <div className="flex items-center justify-between shrink-0">
              <h3 className="text-base font-semibold text-foreground">MCP Servers</h3>
              <button
                type="button"
                title="See docs/reference/mcp/"
                className="p-1 text-muted-foreground hover:text-foreground rounded-md transition"
                aria-label="MCP documentation"
              >
                <HelpCircle className="w-4 h-4" />
              </button>
            </div>

            {/* Quick-toggle chips */}
            <div className="flex flex-wrap gap-2 shrink-0">
              {servers.length === 0 && !mcpLoading && (
                <span className="text-xs text-muted-foreground">No servers configured.</span>
              )}
              {servers.map(s => (
                <button
                  key={s.name}
                  onClick={() => handleToggle(s.name, !s.enabled)}
                  disabled={togglingServer === s.name}
                  className={cn(
                    'flex items-center gap-1.5 px-2.5 py-1 text-xs rounded-full border transition',
                    s.enabled
                      ? 'bg-primary/10 border-primary/30 text-primary'
                      : 'bg-muted border-border text-muted-foreground',
                    togglingServer === s.name && 'opacity-50',
                  )}
                  title={s.enabled ? 'Disable' : 'Enable'}
                >
                  <span className="font-mono">{s.name}</span>
                  {togglingServer === s.name ? (
                    <span className="w-3 h-3 inline-block" />
                  ) : s.enabled ? (
                    <Check className="w-3 h-3" />
                  ) : (
                    <span className="w-3 h-3 leading-none text-center">×</span>
                  )}
                </button>
              ))}
            </div>

            {/* Status line */}
            <div className="text-xs text-muted-foreground shrink-0">
              {servers.length} server{servers.length === 1 ? '' : 's'}
              {disabledCount > 0 && `, ${disabledCount} disabled`}
            </div>

            {mcpError && (
              <div className="flex items-start gap-2 p-3 text-xs text-destructive bg-destructive/10 border border-destructive/30 rounded-md shrink-0">
                <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
                <span className="font-mono whitespace-pre-wrap break-all">{mcpError}</span>
              </div>
            )}

            {/* CodeMirror JSON editor */}
            <div className="flex-1 min-h-0 overflow-hidden border border-border rounded-md">
              <CodeMirror
                value={mcpText}
                onChange={setMcpText}
                extensions={[json()]}
                theme={oneDark}
                height="100%"
                className="h-full text-[13px]"
                basicSetup={{
                  lineNumbers: true,
                  foldGutter: true,
                  highlightActiveLine: true,
                  bracketMatching: true,
                  closeBrackets: true,
                  indentOnInput: true,
                }}
              />
            </div>

            {/* Save / Revert buttons */}
            <div className="flex items-center gap-2 shrink-0">
              <button
                onClick={handleSave}
                disabled={mcpSaving || mcpText === mcpOriginal}
                className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition disabled:opacity-50"
              >
                <Save className="w-3.5 h-3.5" />
                {mcpSaving ? 'Saving...' : 'Save'}
              </button>
              <button
                onClick={handleRevert}
                disabled={mcpText === mcpOriginal}
                className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-foreground bg-secondary hover:bg-accent rounded-md border border-input transition disabled:opacity-50"
              >
                <RotateCcw className="w-3.5 h-3.5" />
                Revert
              </button>
              {mcpSaved && (
                <span className="flex items-center gap-1 text-xs text-green-500">
                  <Check className="w-3.5 h-3.5" />
                  Saved
                </span>
              )}
            </div>

            {/* Quick reference (collapsible) */}
            <div className="shrink-0 border border-border rounded-md">
              <button
                onClick={() => setShowQuickRef(!showQuickRef)}
                className="flex items-center gap-2 w-full px-3 py-2 text-xs font-medium text-foreground bg-panel hover:bg-accent rounded-md transition"
              >
                {showQuickRef ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronRight className="w-3.5 h-3.5" />}
                Quick reference
              </button>
              {showQuickRef && (
                <div className="p-3 space-y-3 border-t border-border">
                  <CopyableExample label="stdio" text={STDIO_EXAMPLE} />
                  <CopyableExample label="http" text={HTTP_EXAMPLE} />
                  <p className="text-xs text-muted-foreground">
                    Environment variables use <code className="font-mono bg-muted px-1 rounded">{'${VAR}'}</code> syntax and are expanded by the backend.
                  </p>
                  <p className="text-xs text-muted-foreground">
                    Compatible with Claude Desktop, Cursor, and Windsurf config files — paste directly into the editor above.
                  </p>
                </div>
              )}
            </div>
          </div>
        )}

        {activeTab === 'general' && (
          <div className="space-y-6">
            <div className="flex items-center gap-2">
              <Settings className="w-4 h-4 text-muted-foreground" />
              <h3 className="text-base font-semibold text-foreground">General Settings</h3>
            </div>
            <p className="text-sm text-muted-foreground">More settings coming soon.</p>
          </div>
        )}
      </div>
    </div>
  )
}

function CopyableExample({ label, text }: { label: string; text: string }) {
  const [copied, setCopied] = useState(false)

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch {
      // ignore clipboard errors
    }
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <span className="text-xs font-mono text-muted-foreground">{label}</span>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1 px-2 py-0.5 text-[10px] text-muted-foreground hover:text-foreground bg-secondary hover:bg-accent rounded border border-border transition"
        >
          {copied ? <Check className="w-3 h-3" /> : <Copy className="w-3 h-3" />}
          {copied ? 'Copied!' : 'Copy'}
        </button>
      </div>
      <pre className="text-[11px] font-mono bg-muted p-2 rounded border border-border overflow-x-auto text-foreground">
        {text}
      </pre>
    </div>
  )
}
