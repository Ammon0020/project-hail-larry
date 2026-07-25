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
  Menu,
} from 'lucide-react'
import type { Agent, McpServerConfig, PromptContextSettings } from '@/types'
import type { ProviderInfo } from '@/lib/api'
import {
  getMcpConfig,
  putMcpConfig,
  patchMcpServer,
  listProviders,
  setProvider,
  disableProvider,
  getPromptContextSettings,
  putPromptContextSettings,
  UnsupportedProvidersError,
} from '@/lib/api'
import { cn } from '@/lib/utils'
import { getStoredTheme, setTheme, type Theme } from '@/lib/theme'
import { ProfilesSettings } from './ProfilesSettings'

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
  activeSessionId,
  activeSection,
  onSectionChange,
}: {
  agents: Agent[]
  onAddAgent: (a: Agent) => Promise<void>
  onDeleteAgent: (id: string) => Promise<void>
  onAutodetect: () => Promise<Agent[]>
  /** Id of the currently active chat session, or null when none is open.
   *  Threaded from App.tsx → EditorPane → here so the Providers (advanced)
   *  section can call the session-scoped provider endpoints. When null the
   *  section renders a muted "open a session" hint instead of fetching. */
  activeSessionId?: string | null
  /**
   * Controlled settings section (Agents / MCP / General). Owned by App so
   * deep-links (e.g. MCP popout Settings icon) can focus a section.
   */
  activeSection?: 'agents' | 'mcp' | 'general' | 'profiles'
  /** Called when the user picks a different settings section. */
  onSectionChange?: (section: 'agents' | 'mcp' | 'general' | 'profiles') => void
}) {
  // Prefer controlled section from App when provided; otherwise local state
  // (keeps the panel usable in isolation / Storybook).
  const [localTab, setLocalTab] = useState<'agents' | 'mcp' | 'general' | 'profiles'>('agents')
  const activeTab = activeSection ?? localTab
  const setActiveTab = onSectionChange ?? setLocalTab
  const [isDetecting, setIsDetecting] = useState(false)
  const [showMobileNav, setShowMobileNav] = useState(false)
  const [localTheme, setLocalTheme] = useState<Theme>(getStoredTheme())

  // New agent form state
  const [showAddForm, setShowAddForm] = useState(false)
  const [newAgent, setNewAgent] = useState<Partial<Agent>>({ models: [] })
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

  // Host-wide prompt limits live in config.toml instead of profiles because
  // they govern how much local workspace state any agent can receive.
  const [promptContext, setPromptContext] = useState<PromptContextSettings | null>(null)
  const [promptContextOriginal, setPromptContextOriginal] = useState<PromptContextSettings | null>(null)
  const [promptContextLoading, setPromptContextLoading] = useState(true)
  const [promptContextSaving, setPromptContextSaving] = useState(false)
  const [promptContextError, setPromptContextError] = useState<string | null>(null)

  // Providers (advanced) state — capability-gated per active session.
  // `providersStatus` discriminates loading / unsupported / loaded / error so
  // the section can render the right muted note without conflating a 501
  // (agent lacks provider support) with a transport failure.
  const [providers, setProviders] = useState<ProviderInfo[]>([])
  const [providersStatus, setProvidersStatus] = useState<
    'idle' | 'loading' | 'unsupported' | 'loaded' | 'error'
  >('idle')
  const [providersError, setProvidersError] = useState<string | null>(null)
  const [providerBusy, setProviderBusy] = useState<string | null>(null)

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
    await onAddAgent(newAgent as Agent)
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

  useEffect(() => {
    void loadPromptContext()
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

  async function loadPromptContext() {
    setPromptContextLoading(true)
    try {
      const settings = await getPromptContextSettings()
      setPromptContext(settings)
      setPromptContextOriginal(settings)
      setPromptContextError(null)
    } catch (error) {
      setPromptContextError(error instanceof Error ? error.message : String(error))
    } finally {
      setPromptContextLoading(false)
    }
  }

  async function savePromptContext() {
    if (!promptContext) return
    setPromptContextSaving(true)
    try {
      const settings = await putPromptContextSettings(promptContext)
      setPromptContext(settings)
      setPromptContextOriginal(settings)
      setPromptContextError(null)
    } catch (error) {
      setPromptContextError(error instanceof Error ? error.message : String(error))
    } finally {
      setPromptContextSaving(false)
    }
  }

  function updatePromptContext(key: keyof PromptContextSettings, value: string) {
    const number = Number(value)
    if (!Number.isInteger(number) || number < 0 || number > 100) return
    setPromptContext((current) => (current ? { ...current, [key]: number } : current))
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

  // ---- Providers (advanced) ----

  /** Fetches the provider list for the active session. A 501 maps to the
   *  `unsupported` status (rendered as a muted note); any other failure
   *  surfaces inline via `providersError`. Safe to call with no active
   *  session — returns early leaving the section idle. */
  async function loadProviders(sessionId: string) {
    setProvidersStatus('loading')
    setProvidersError(null)
    try {
      const list = await listProviders(sessionId)
      setProviders(list)
      setProvidersStatus('loaded')
    } catch (e) {
      if (e instanceof UnsupportedProvidersError) {
        setProvidersStatus('unsupported')
        setProviders([])
      } else {
        setProvidersStatus('error')
        setProvidersError(e instanceof Error ? e.message : String(e))
      }
    }
  }

  // Load providers when the General tab is opened with an active session.
  // Re-fetches when the session changes so switching chats refreshes the list.
  // Mirrors the loadMcp effect above: the async helper sets a 'loading' state
  // before its first await (required for the loading indicator). The
  // set-state-in-effect rule flags interprocedural calls with args but not
  // the arg-less loadMcp — same pattern, so we disable it here for parity.
  useEffect(() => {
    if (activeTab !== 'general' || !activeSessionId) return
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadProviders(activeSessionId)
  }, [activeTab, activeSessionId])

  /** Parses a "key: value" textarea into a header record. Blank lines and
   *  lines without a colon are skipped. Values are trimmed but otherwise
   *  passed through verbatim — they may contain auth tokens, so we never
   *  log the parsed result. */
  function parseHeaders(text: string): Record<string, string> {
    const out: Record<string, string> = {}
    for (const raw of text.split('\n')) {
      const line = raw.trim()
      if (!line) continue
      const idx = line.indexOf(':')
      if (idx <= 0) continue
      const key = line.slice(0, idx).trim()
      const val = line.slice(idx + 1).trim()
      if (key) out[key] = val
    }
    return out
  }

  async function handleSetProvider(
    providerId: string,
    apiType: string,
    baseUrl: string,
    headersText: string,
  ) {
    if (!activeSessionId) return
    setProviderBusy(providerId)
    setProvidersError(null)
    try {
      await setProvider(
        activeSessionId,
        providerId,
        apiType,
        baseUrl,
        parseHeaders(headersText),
      )
      await loadProviders(activeSessionId)
    } catch (e) {
      setProvidersError(e instanceof Error ? e.message : String(e))
    } finally {
      setProviderBusy(null)
    }
  }

  async function handleDisableProvider(providerId: string) {
    if (!activeSessionId) return
    setProviderBusy(providerId)
    setProvidersError(null)
    try {
      await disableProvider(activeSessionId, providerId)
      await loadProviders(activeSessionId)
    } catch (e) {
      setProvidersError(e instanceof Error ? e.message : String(e))
    } finally {
      setProviderBusy(null)
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
    <div className="h-full flex flex-col md:flex-row relative">
      {/* Mobile Header */}
      <div className="md:hidden flex items-center justify-between p-3 border-b border-border bg-panel shrink-0">
        <div className="flex items-center gap-2">
          <button 
            onClick={() => setShowMobileNav(!showMobileNav)} 
            className="p-1.5 hover:bg-accent rounded-md transition"
            aria-label="Toggle navigation menu"
            aria-expanded={showMobileNav}
          >
            <Menu className="w-5 h-5 text-foreground" />
          </button>
          <span className="font-semibold text-sm">
            {activeTab === 'agents' ? 'Agents & Models' : activeTab === 'mcp' ? 'MCP Servers' : activeTab === 'profiles' ? 'Profiles' : 'General Settings'}
          </span>
        </div>
      </div>

      {/* Mobile Nav Overlay */}
      <div 
        className={cn(
          "md:hidden absolute inset-x-0 bottom-0 top-[53px] z-50 bg-background flex flex-col p-2 transition-all duration-200 ease-out origin-top",
          showMobileNav ? "opacity-100 scale-y-100" : "opacity-0 scale-y-95 pointer-events-none"
        )}
      >
        <button
          onClick={() => { setActiveTab('agents'); setShowMobileNav(false) }}
          className={tabButtonClass(activeTab === 'agents')}
        >
          Agents & Models
        </button>
        <button
          onClick={() => { setActiveTab('mcp'); setShowMobileNav(false) }}
          className={tabButtonClass(activeTab === 'mcp')}
        >
          MCP Servers
        </button>
        <button
          onClick={() => { setActiveTab('general'); setShowMobileNav(false) }}
          className={tabButtonClass(activeTab === 'general')}
        >
          General
        </button>
        <button
          onClick={() => { setActiveTab('profiles'); setShowMobileNav(false) }}
          className={tabButtonClass(activeTab === 'profiles')}
        >
          Profiles
        </button>
      </div>

      {/* Sidebar - Desktop Only */}
      <div className="hidden md:flex w-48 bg-activity-bar border-r border-border flex-col py-2 shrink-0">
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
        <button
          onClick={() => setActiveTab('profiles')}
          className={tabButtonClass(activeTab === 'profiles')}
        >
          Profiles
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
            
            {/* Theme Section */}
            <div className="p-4 bg-panel border border-border rounded-lg space-y-3">
              <h4 className="font-semibold text-sm text-foreground">Theme</h4>
              <p className="text-xs text-muted-foreground">Choose the visual appearance of the application.</p>
              
              <div className="flex flex-col gap-2 mt-2">
                {(['dark', 'light', 'system'] as Theme[]).map(t => (
                  <label key={t} className="flex items-center gap-2 cursor-pointer w-fit">
                    <input 
                      type="radio" 
                      name="theme" 
                      value={t} 
                      checked={localTheme === t}
                      onChange={() => {
                        setLocalTheme(t)
                        setTheme(t)
                      }}
                      className="text-primary focus:ring-primary h-4 w-4 border-input accent-primary cursor-pointer"
                    />
                    <span className="text-sm capitalize text-foreground">{t}</span>
                  </label>
                ))}
              </div>
            </div>

            <div className="p-4 bg-panel border border-border rounded-lg space-y-3">
              <div>
                <h4 className="font-semibold text-sm text-foreground">Prompt context</h4>
                <p className="mt-1 text-xs text-muted-foreground">
                  Only relative paths are added automatically. File contents are never added from open tabs; explicit editor selections remain separate context.
                </p>
              </div>
              {promptContextError && (
                <div className="flex items-start gap-2 p-2 text-xs text-destructive bg-destructive/10 border border-destructive/30 rounded-md">
                  <AlertTriangle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
                  <span>{promptContextError}</span>
                </div>
              )}
              {promptContextLoading || !promptContext ? (
                <p className="text-xs text-muted-foreground">Loading context settings…</p>
              ) : (
                <>
                  <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                    <label className="space-y-1">
                      <span className="block text-xs text-foreground">Open and recently edited paths</span>
                      <input
                        type="number"
                        min={0}
                        max={100}
                        value={promptContext.openFileLimit}
                        onChange={(event) => updatePromptContext('openFileLimit', event.target.value)}
                        className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm"
                      />
                    </label>
                    <label className="space-y-1">
                      <span className="block text-xs text-foreground">Top-level workspace entries</span>
                      <input
                        type="number"
                        min={0}
                        max={100}
                        value={promptContext.workspaceFileListLimit}
                        onChange={(event) => updatePromptContext('workspaceFileListLimit', event.target.value)}
                        className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm"
                      />
                    </label>
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={savePromptContext}
                      disabled={promptContextSaving || JSON.stringify(promptContext) === JSON.stringify(promptContextOriginal)}
                      className="px-3 py-1.5 text-xs font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition disabled:opacity-50"
                    >
                      {promptContextSaving ? 'Saving…' : 'Save context limits'}
                    </button>
                    <span className="text-[11px] text-muted-foreground">0 disables a list; maximum 100.</span>
                  </div>
                </>
              )}
            </div>

            {/* Providers (advanced) — per-session ACP provider configuration.
                Capability-gated: hidden entirely when no session is open, and
                shows a muted note when the agent returns 501 (no provider
                support). Renders one ProviderRow per provider. */}
            <div className="p-4 bg-panel border border-border rounded-lg space-y-3">
              <div className="flex items-center gap-2">
                <h4 className="font-semibold text-sm text-foreground">Providers (advanced)</h4>
                {providersStatus === 'loading' && (
                  <span className="text-xs text-muted-foreground">Loading…</span>
                )}
              </div>
              <p className="text-xs text-muted-foreground">
                Configure the model providers the active session's agent can use. Per-session; advanced.
              </p>

              {!activeSessionId && (
                <p className="text-xs text-muted-foreground italic">
                  Open a chat session to configure providers.
                </p>
              )}

              {activeSessionId && providersStatus === 'unsupported' && (
                <p className="text-xs text-muted-foreground italic">
                  The current agent does not support runtime provider configuration.
                </p>
              )}

              {activeSessionId && providersStatus === 'error' && providersError && (
                <div className="flex items-start gap-2 p-3 text-xs text-destructive bg-destructive/10 border border-destructive/30 rounded-md">
                  <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
                  <span className="font-mono whitespace-pre-wrap break-all">{providersError}</span>
                </div>
              )}

              {activeSessionId && providersStatus === 'loaded' && providers.length === 0 && (
                <p className="text-xs text-muted-foreground italic">
                  No configurable providers for this agent.
                </p>
              )}

              {activeSessionId && (providersStatus === 'loaded' || providersStatus === 'loading') && providers.length > 0 && (
                <div className="space-y-3">
                  {providers.map(p => (
                    <ProviderRow
                      key={`${p.id}:${p.current?.apiType ?? ''}:${p.current?.baseUrl ?? ''}`}
                      provider={p}
                      busy={providerBusy === p.id}
                      error={providerBusy === p.id ? providersError : null}
                      onSet={(apiType, baseUrl, headersText) =>
                        handleSetProvider(p.id, apiType, baseUrl, headersText)
                      }
                      onDisable={() => handleDisableProvider(p.id)}
                    />
                  ))}
                </div>
              )}
            </div>
          </div>
        )}

        {activeTab === 'profiles' && <ProfilesSettings />}
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

/**
 * One row per provider in the Providers (advanced) section. Shows the
 * provider id, a "required" badge, the supported protocols, and the current
 * config (or "disabled"). Renders a small inline form to set apiType
 * (dropdown constrained to `supported`), baseUrl, optional headers
 * (key:value textarea), and a Disable button — hidden when the provider is
 * required (the backend rejects DELETE with 400 for required providers).
 *
 * Headers may contain auth tokens, so the textarea value is never logged.
 */
function ProviderRow({
  provider,
  busy,
  error,
  onSet,
  onDisable,
}: {
  provider: ProviderInfo
  busy: boolean
  error: string | null
  onSet: (apiType: string, baseUrl: string, headersText: string) => void
  onDisable: () => void
}) {
  const supported = provider.supported.length > 0 ? provider.supported : []
  const initialApiType = provider.current?.apiType ?? supported[0] ?? ''
  const [apiType, setApiType] = useState(initialApiType)
  const [baseUrl, setBaseUrl] = useState(provider.current?.baseUrl ?? '')
  const [headersText, setHeadersText] = useState('')

  // The parent remounts this row (via a `key` that includes current.apiType
  // and current.baseUrl) after a successful set/disable round-trip, so the
  // useState initializers above re-run with the refreshed config — no
  // sync-setState effect needed.

  const canSet = !busy && apiType && baseUrl.trim().length > 0
  const canDisable = !busy && !provider.required && !!provider.current

  return (
    <div className="p-3 bg-background border border-border rounded-md space-y-2">
      <div className="flex items-center gap-2 flex-wrap">
        <span className="font-mono text-sm text-foreground">{provider.id}</span>
        {provider.required && (
          <span className="text-[10px] px-1.5 py-0.5 rounded border border-amber-500/30 bg-amber-500/10 text-amber-600 dark:text-amber-400">
            required
          </span>
        )}
        {supported.length > 0 && (
          <span className="text-[10px] text-muted-foreground">
            supports: {supported.join(', ')}
          </span>
        )}
      </div>

      <div className="text-xs text-muted-foreground">
        {provider.current
          ? <>current: <span className="font-mono">{provider.current.apiType}</span> · <span className="font-mono break-all">{provider.current.baseUrl}</span></>
          : <span className="italic">disabled</span>}
      </div>

      <div className="grid grid-cols-1 md:grid-cols-[120px_1fr] gap-2">
        <select
          value={apiType}
          onChange={e => setApiType(e.target.value)}
          disabled={busy || supported.length === 0}
          className="bg-background border border-input rounded-md px-2 py-1.5 text-sm disabled:opacity-50"
          aria-label={`${provider.id} api type`}
        >
          {supported.length === 0 && <option value="">—</option>}
          {supported.map(t => (
            <option key={t} value={t}>{t}</option>
          ))}
        </select>
        <input
          type="text"
          value={baseUrl}
          onChange={e => setBaseUrl(e.target.value)}
          placeholder="https://api.example.com"
          disabled={busy}
          className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm disabled:opacity-50"
          aria-label={`${provider.id} base URL`}
        />
      </div>

      <textarea
        value={headersText}
        onChange={e => setHeadersText(e.target.value)}
        placeholder={'Optional headers (one per line):\nAuthorization: Bearer ...'}
        rows={2}
        disabled={busy}
        className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-xs font-mono disabled:opacity-50"
        aria-label={`${provider.id} headers`}
      />

      {error && (
        <div className="flex items-start gap-2 p-2 text-xs text-destructive bg-destructive/10 border border-destructive/30 rounded-md">
          <AlertTriangle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
          <span className="font-mono whitespace-pre-wrap break-all">{error}</span>
        </div>
      )}

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={() => onSet(apiType, baseUrl.trim(), headersText)}
          disabled={!canSet}
          className="px-3 py-1.5 text-xs font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition disabled:opacity-50"
        >
          {busy ? 'Saving…' : 'Set'}
        </button>
        {provider.required ? (
          <span className="text-[10px] text-muted-foreground">Required providers cannot be disabled.</span>
        ) : (
          <button
            type="button"
            onClick={onDisable}
            disabled={!canDisable}
            className="px-3 py-1.5 text-xs font-medium text-foreground bg-secondary hover:bg-accent rounded-md border border-input transition disabled:opacity-50"
          >
            Disable
          </button>
        )}
      </div>
    </div>
  )
}
