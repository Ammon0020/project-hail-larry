import { useState } from 'react'
import { Search, Plus, Trash2, AlertTriangle, Monitor } from 'lucide-react'
import type { AgentInfo } from '@/lib/api'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

export function SettingsModal({
  isOpen,
  onClose,
  agents,
  onAddAgent,
  onDeleteAgent,
  onAutodetect,
}: {
  isOpen: boolean
  onClose: () => void
  agents: AgentInfo[]
  onAddAgent: (a: AgentInfo) => Promise<void>
  onDeleteAgent: (id: string) => Promise<void>
  onAutodetect: () => Promise<AgentInfo[]>
}) {
  const [activeTab, setActiveTab] = useState<'agents' | 'general'>('agents')
  const [isDetecting, setIsDetecting] = useState(false)
  
  // New agent form state
  const [showAddForm, setShowAddForm] = useState(false)
  const [newAgent, setNewAgent] = useState<Partial<AgentInfo>>({ models: [] })
  const [newModel, setNewModel] = useState({ id: '', name: '' })

  // Escape, click-outside, focus trap, and body-scroll locking are all handled
  // by the underlying Radix Dialog (see @/components/ui/dialog).
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
      models: [...(prev.models || []), { ...newModel }]
    }))
    setNewModel({ id: '', name: '' })
  }

  return (
    <Dialog open={isOpen} onOpenChange={(o) => { if (!o) onClose() }}>
      <DialogContent
        aria-describedby={undefined}
        className="max-w-2xl p-0 gap-0 flex flex-col max-h-[85vh] overflow-hidden"
      >
        {/* Header */}
        <DialogHeader className="px-5 py-4 border-b border-border">
          <DialogTitle className="text-lg font-bold text-foreground">Settings</DialogTitle>
        </DialogHeader>

        <div className="flex flex-1 overflow-hidden">
          {/* Sidebar */}
          <div className="w-48 bg-activity-bar border-r border-border flex flex-col py-2 shrink-0">
            <button
              onClick={() => setActiveTab('agents')}
              className={`px-4 py-2 text-sm text-left transition ${activeTab === 'agents' ? 'text-primary bg-primary/10 font-medium border-r-2 border-primary' : 'text-muted-foreground hover:text-foreground'}`}
            >
              Agents & Models
            </button>
            <button
              onClick={() => setActiveTab('general')}
              className={`px-4 py-2 text-sm text-left transition ${activeTab === 'general' ? 'text-primary bg-primary/10 font-medium border-r-2 border-primary' : 'text-muted-foreground hover:text-foreground'}`}
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
                          onChange={e => setNewAgent({...newAgent, id: e.target.value})}
                          className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm"
                        />
                      </div>
                      <div>
                        <label htmlFor="agent-name-input" className="block text-xs text-muted-foreground mb-1">Name (e.g., Custom CLI)</label>
                        <input
                          id="agent-name-input"
                          type="text"
                          value={newAgent.name || ''}
                          onChange={e => setNewAgent({...newAgent, name: e.target.value})}
                          className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm"
                        />
                      </div>
                      <div className="col-span-2">
                        <label htmlFor="agent-command-input" className="block text-xs text-muted-foreground mb-1">Command executable</label>
                        <input
                          id="agent-command-input"
                          type="text"
                          value={newAgent.command || ''}
                          onChange={e => setNewAgent({...newAgent, command: e.target.value})}
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
                          onChange={e => setNewModel({...newModel, id: e.target.value})}
                          className="flex-1 bg-background border border-input rounded-md px-3 py-1.5 text-sm"
                        />
                        <label htmlFor="model-name-input" className="sr-only">Model name</label>
                        <input
                          id="model-name-input"
                          type="text"
                          placeholder="Model Name"
                          value={newModel.name}
                          onChange={e => setNewModel({...newModel, name: e.target.value})}
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

            {activeTab === 'general' && (
              <div className="space-y-6">
                <h3 className="text-base font-semibold text-foreground">General Settings</h3>
                <p className="text-sm text-muted-foreground">More settings coming soon.</p>
              </div>
            )}

          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
