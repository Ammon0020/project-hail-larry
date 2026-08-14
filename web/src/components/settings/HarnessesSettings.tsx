import { useState } from 'react'
import { AlertTriangle, Monitor, Plus, Search, Trash2 } from 'lucide-react'
import type { Agent } from '@/types'
import { ErrorNote, LabeledInput } from './shared'

export function HarnessesSettings({ agents, onAddAgent, onDeleteAgent, onAutodetect }: {
  agents: Agent[]
  onAddAgent: (agent: Agent) => Promise<void>
  onDeleteAgent: (id: string) => Promise<void>
  onAutodetect: () => Promise<Agent[]>
}) {
  const [isDetecting, setIsDetecting] = useState(false)
  const [showAddForm, setShowAddForm] = useState(false)
  const [newAgent, setNewAgent] = useState<Partial<Agent>>({ models: [] })
  const [newModel, setNewModel] = useState({ id: '', name: '' })
  const [addAgentError, setAddAgentError] = useState<string | null>(null)
  const handleAutodetect = async () => {
    setIsDetecting(true)
    try {
      const detected = await onAutodetect()
      for (const agent of detected) {
        const existing = agents.find(current => current.id === agent.id)
        await onAddAgent(existing ? { ...existing, models: agent.models, command: agent.command } : agent)
      }
    } catch (error) {
      console.error(error)
    } finally {
      setIsDetecting(false)
    }
  }
  const handleAddAgent = async () => {
    setAddAgentError(null)
    if (!newAgent.id || !newAgent.name || !newAgent.command) {
      setAddAgentError('ID, name, and command are all required.')
      return
    }
    try {
      await onAddAgent(newAgent as Agent)
      setShowAddForm(false)
      setNewAgent({ models: [] })
    } catch (error: unknown) {
      setAddAgentError(error instanceof Error ? error.message : String(error))
    }
  }
  const handleAddModel = () => {
    if (!newModel.id || !newModel.name) return
    setNewAgent(current => ({ ...current, models: [...(current.models || []), { ...newModel }] }))
    setNewModel({ id: '', name: '' })
  }
  return (
    <section id="harnesses" className="scroll-mt-4 space-y-6">
      <div className="flex items-center justify-between">
        <h3 className="text-base font-semibold text-foreground">Configured Agents</h3>
        <div className="flex gap-2">
          <button onClick={handleAutodetect} disabled={isDetecting} className="flex items-center gap-2 rounded-md border border-input bg-secondary px-3 py-1.5 text-xs font-medium text-foreground transition hover:bg-accent disabled:opacity-50"><Search className="size-3.5" />{isDetecting ? 'Detecting...' : 'Auto-detect'}</button>
          <button onClick={() => setShowAddForm(!showAddForm)} className="flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition hover:bg-primary/90"><Plus className="size-3.5" />Add Custom</button>
        </div>
      </div>
      {showAddForm && <div className="space-y-4 rounded-lg border border-primary/30 bg-panel p-4">
        <div className="grid grid-cols-2 gap-4">
          <LabeledInput id="agent-id-input" label="ID (e.g., custom-agent)" value={newAgent.id || ''} onChange={value => setNewAgent({ ...newAgent, id: value })} />
          <LabeledInput id="agent-name-input" label="Name (e.g., Custom CLI)" value={newAgent.name || ''} onChange={value => setNewAgent({ ...newAgent, name: value })} />
          <LabeledInput id="agent-command-input" label="Command executable" wrapperClass="col-span-2" value={newAgent.command || ''} onChange={value => setNewAgent({ ...newAgent, command: value })} placeholder="e.g. claude, codex, or /absolute/path/to/bin" />
        </div>
        <div className="border-t border-border pt-3">
          <label htmlFor="model-id-input" className="mb-2 block text-xs text-muted-foreground">Models</label>
          <div className="mb-2 space-y-2">{newAgent.models?.map((model, index) => <div key={index} className="flex items-center gap-2 rounded border border-border bg-background p-2 text-xs"><Monitor className="size-3 text-muted-foreground" /><span className="font-mono text-primary">{model.id}</span><span className="text-muted-foreground">({model.name})</span></div>)}</div>
          <div className="flex gap-2">
            <input id="model-id-input" type="text" placeholder="Model ID" value={newModel.id} onChange={event => setNewModel({ ...newModel, id: event.target.value })} className="flex-1 rounded-md border border-input bg-background px-3 py-1.5 text-sm" />
            <label htmlFor="model-name-input" className="sr-only">Model name</label>
            <input id="model-name-input" type="text" placeholder="Model Name" value={newModel.name} onChange={event => setNewModel({ ...newModel, name: event.target.value })} className="flex-1 rounded-md border border-input bg-background px-3 py-1.5 text-sm" />
            <button onClick={handleAddModel} className="rounded-md bg-secondary px-3 py-1.5 text-xs hover:bg-accent">Add Model</button>
          </div>
        </div>
        {addAgentError && <ErrorNote message={addAgentError} />}
        <div className="flex justify-end pt-2"><button onClick={handleAddAgent} className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90">Save Agent</button></div>
      </div>}
      <div className="space-y-3">{agents.map(agent => <div key={agent.id} className="group flex flex-col gap-3 rounded-lg border border-border bg-panel p-4">
        <div className="flex items-start justify-between"><div><h4 className="font-semibold text-foreground">{agent.name}</h4><div className="mt-1 flex items-center gap-2"><span className="rounded border border-border bg-muted px-1.5 py-0.5 font-mono text-xs text-muted-foreground">{agent.command}</span>{agent.warning && <span className="flex items-center gap-1 rounded border border-amber-500/20 bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-500"><AlertTriangle className="size-3" />{agent.warning}</span>}</div></div>
          <button onClick={() => onDeleteAgent(agent.id)} title="Delete Agent" aria-label={`Delete agent ${agent.name}`} className="rounded-md p-1.5 text-muted-foreground opacity-0 transition group-hover:opacity-100 hover:bg-destructive/10 hover:text-destructive"><Trash2 className="size-4" /></button>
        </div><div className="flex flex-wrap gap-2">{agent.models.map(model => <span key={model.id} className="rounded-md border border-border bg-background px-2 py-1 text-xs text-muted-foreground">{model.name}</span>)}</div>
      </div>)}{agents.length === 0 && <div className="rounded-lg border border-dashed border-input p-8 text-center text-sm text-muted-foreground">No agents configured. Click Auto-detect to search for installed agents.</div>}</div>
    </section>
  )
}
