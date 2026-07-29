import { useCallback, useEffect, useRef, useState } from 'react'
import { ChevronDown, ChevronRight, Menu, Search, Settings } from 'lucide-react'
import type { Agent } from '@/types'
import { cn } from '@/lib/utils'
import { getStoredTheme, setTheme, type Theme } from '@/lib/theme'
import type { EditorSettings as EditorSettingsState } from '@/hooks/useEditorSettings'
import { ProfilesSettings } from './ProfilesSettings'
import { EditorSettings } from './settings/EditorSettings'
import { HarnessesSettings } from './settings/HarnessesSettings'
import { McpServersSettings } from './settings/McpServersSettings'
import { PreviewTrustSettings } from './settings/PreviewTrustSettings'
import { PromptContextSettings } from './settings/PromptContextSettings'
import { ProvidersSettings } from './settings/ProvidersSettings'

export type SettingsSection =
  | 'theme' | 'editor'
  | 'harnesses' | 'profiles' | 'prompt-context' | 'providers'
  | 'mcp-servers'
  | 'preview' | 'permissions'
  | 'connection' | 'pairing' | 'security'

const SECTION_LABELS: Record<SettingsSection, string> = {
  theme: 'Theme', editor: 'Editor', harnesses: 'Harnesses', profiles: 'Profiles',
  'prompt-context': 'Prompt Context', providers: 'Providers', 'mcp-servers': 'MCP Servers',
  preview: 'Preview', permissions: 'Permissions', connection: 'Connection', pairing: 'Pairing', security: 'Security',
}

const NAV_GROUPS: { label: string; sections: SettingsSection[] }[] = [
  { label: 'Appearance', sections: ['theme', 'editor'] },
  { label: 'Agents & AI', sections: ['harnesses', 'profiles', 'prompt-context', 'providers'] },
  { label: 'Tools', sections: ['mcp-servers'] },
  { label: 'Workspace', sections: ['preview', 'permissions'] },
  { label: 'Server & Network', sections: ['connection', 'pairing', 'security'] },
]

const SERVER_NETWORK_PLACEHOLDERS: ReadonlyArray<readonly [id: SettingsSection, label: string, desc: string]> = [
  ['connection', 'Connection', 'Server configuration'],
  ['pairing', 'Pairing', 'Device pairing configuration'],
  ['security', 'Security', 'Security and TLS configuration'],
]

export function SettingsPanel({
  agents, onAddAgent, onDeleteAgent, onAutodetect, activeSessionId, activeSection,
  onSectionChange, workspaceId, workspaceTrusted, onSetWorkspaceTrust, editorSettings,
  onEditorSettingsChange,
}: {
  agents: Agent[]
  onAddAgent: (agent: Agent) => Promise<void>
  onDeleteAgent: (id: string) => Promise<void>
  onAutodetect: () => Promise<Agent[]>
  activeSessionId?: string | null
  activeSection?: SettingsSection
  onSectionChange?: (section: SettingsSection) => void
  workspaceId?: string
  workspaceTrusted?: boolean | null
  onSetWorkspaceTrust?: (workspaceId: string, trusted: boolean | null) => Promise<void>
  editorSettings?: EditorSettingsState
  onEditorSettingsChange?: (patch: Partial<EditorSettingsState>) => void
}) {
  const [activeView, setActiveView] = useState<SettingsSection>(activeSection ?? 'harnesses')
  const [showMobileNav, setShowMobileNav] = useState(false)
  const [localTheme, setLocalTheme] = useState<Theme>(getStoredTheme())
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set())
  const [searchQuery, setSearchQuery] = useState('')
  const scrollContainerRef = useRef<HTMLDivElement>(null)

  const scrollToSection = useCallback((section: SettingsSection) => {
    document.getElementById(section)?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }, [])

  useEffect(() => { if (activeSection) scrollToSection(activeSection) }, [activeSection, scrollToSection])
  useEffect(() => {
    const container = scrollContainerRef.current
    if (!container) return
    const observer = new IntersectionObserver(entries => {
      for (const entry of entries) if (entry.isIntersecting) {
        const section = entry.target.id as SettingsSection
        setActiveView(section)
        onSectionChange?.(section)
      }
    }, { root: container, rootMargin: '-10% 0px -80% 0px', threshold: 0 })
    container.querySelectorAll<HTMLElement>('section[id]').forEach(section => observer.observe(section))
    return () => observer.disconnect()
  }, [onSectionChange])

  const toggleGroup = useCallback((label: string) => {
    setCollapsedGroups(current => {
      const next = new Set(current)
      if (next.has(label)) next.delete(label)
      else next.add(label)
      return next
    })
  }, [])
  const query = searchQuery.trim().toLowerCase()
  const isSearching = query.length > 0
  const subButtonClass = (active: boolean) => cn('pl-7 pr-4 py-1.5 text-sm text-left transition w-full', active ? 'text-primary bg-primary/10 font-medium border-r-2 border-primary' : 'text-muted-foreground hover:text-foreground')
  const renderNav = () => <div className="flex flex-col py-2">
    <div className="px-3 pb-2 relative"><Search className="absolute left-5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground pointer-events-none" /><input type="text" value={searchQuery} onChange={event => setSearchQuery(event.target.value)} placeholder="Search settings" aria-label="Search settings" className="w-full pl-7 pr-2 py-1.5 text-xs bg-background border border-input rounded-md text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-primary" /></div>
    {NAV_GROUPS.map(group => {
      const visible = isSearching ? group.sections.filter(section => SECTION_LABELS[section].toLowerCase().includes(query)) : group.sections
      if (isSearching && visible.length === 0) return null
      const expanded = isSearching || !collapsedGroups.has(group.label)
      return <div key={group.label} className="mb-1"><button onClick={() => toggleGroup(group.label)} aria-expanded={expanded} aria-label={`Toggle ${group.label} group`} className="flex items-center gap-1 w-full px-3 pt-2 pb-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground/70 hover:text-foreground transition">{expanded ? <ChevronDown className="w-3 h-3 shrink-0" /> : <ChevronRight className="w-3 h-3 shrink-0" />}{group.label}</button>{expanded && visible.map(section => <button key={section} onClick={() => { scrollToSection(section); setShowMobileNav(false) }} className={subButtonClass(activeView === section)}>{SECTION_LABELS[section]}</button>)}</div>
    })}
  </div>

  return <div className="h-full flex flex-col md:flex-row relative">
    <div className="md:hidden flex items-center justify-between p-3 border-b border-border bg-panel shrink-0"><div className="flex items-center gap-2"><button onClick={() => setShowMobileNav(!showMobileNav)} aria-label="Toggle navigation menu" aria-expanded={showMobileNav} className="p-1.5 hover:bg-accent rounded-md transition"><Menu className="w-5 h-5 text-foreground" /></button><span className="font-semibold text-sm">{SECTION_LABELS[activeView]}</span></div></div>
    <div className={cn('md:hidden absolute inset-x-0 bottom-0 top-[53px] z-50 bg-background flex flex-col p-2 overflow-y-auto transition-all duration-200 ease-out origin-top', showMobileNav ? 'opacity-100 scale-y-100' : 'opacity-0 scale-y-95 pointer-events-none')}>{renderNav()}</div>
    <div className="hidden md:flex w-56 bg-activity-bar border-r border-border flex-col overflow-y-auto shrink-0">{renderNav()}</div>
    <div ref={scrollContainerRef} className="flex-1 overflow-y-auto p-5 bg-background">
      <section id="theme" className="scroll-mt-4 space-y-6"><div className="flex items-center gap-2"><Settings className="w-4 h-4 text-muted-foreground" /><h3 className="text-base font-semibold text-foreground">Theme</h3></div><div className="p-4 bg-panel border border-border rounded-lg space-y-3"><h4 className="font-semibold text-sm text-foreground">Appearance</h4><p className="text-xs text-muted-foreground">Choose the visual appearance of the application.</p><div className="flex flex-col gap-2 mt-2">{(['dark', 'light', 'system'] as Theme[]).map(theme => <label key={theme} className="flex items-center gap-2 cursor-pointer w-fit"><input type="radio" name="theme" value={theme} checked={localTheme === theme} onChange={() => { setLocalTheme(theme); setTheme(theme) }} className="text-primary focus:ring-primary h-4 w-4 border-input accent-primary cursor-pointer" /><span className="text-sm capitalize text-foreground">{theme}</span></label>)}</div></div></section>
      <section id="editor" className="scroll-mt-4">{editorSettings && onEditorSettingsChange ? <EditorSettings settings={editorSettings} onChange={onEditorSettingsChange} /> : <div className="p-6 text-sm text-muted-foreground"><p>Editor settings unavailable.</p></div>}</section>
      <HarnessesSettings agents={agents} onAddAgent={onAddAgent} onDeleteAgent={onDeleteAgent} onAutodetect={onAutodetect} />
      <section id="profiles" className="scroll-mt-4"><ProfilesSettings /></section>
      <PromptContextSettings />
      <ProvidersSettings active={activeView === 'providers'} activeSessionId={activeSessionId} />
      <McpServersSettings />
      <PreviewTrustSettings workspaceId={workspaceId} workspaceTrusted={workspaceTrusted} onSetWorkspaceTrust={onSetWorkspaceTrust} />
      <section id="permissions" className="scroll-mt-4 p-6 text-sm text-muted-foreground space-y-2"><h3 className="text-base font-semibold text-foreground">Permissions</h3><p>Coming soon — per-workspace permission policies for file writes, shell commands, and network access will be configured here.</p></section>
      {SERVER_NETWORK_PLACEHOLDERS.map(([id, label, description]) => <section key={id} id={id} className="scroll-mt-4 p-6 text-sm text-muted-foreground space-y-2"><h3 className="text-base font-semibold text-foreground">{label}</h3><p>{description} — coming soon.</p><p className="text-xs">These settings require editing config.toml and restarting the daemon.</p></section>)}
    </div>
  </div>
}
