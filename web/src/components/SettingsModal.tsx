import { useState } from 'react'
import { X, Search, Plus, Trash2, AlertTriangle, Monitor } from 'lucide-react'
import type { AgentInfo } from '@/lib/api'

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

  if (!isOpen) return null

  const handleAutodetect = async () => {
    setIsDetecting(true)
    try {
      const detected = await onAutodetect()
      for (const d of detected) {
        if (!agents.find(a => a.id === d.id)) {
          await onAddAgent(d)
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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4">
      <div className="bg-panel border border-gray-700 rounded-xl shadow-2xl w-full max-w-2xl flex flex-col max-h-[85vh] overflow-hidden animate-in fade-in zoom-in-95 duration-200">
        
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-gray-800">
          <h2 className="text-lg font-bold text-gray-200">Settings</h2>
          <button onClick={onClose} className="p-1 text-gray-500 hover:text-gray-300 rounded-md transition">
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="flex flex-1 overflow-hidden">
          {/* Sidebar */}
          <div className="w-48 bg-activity-bar border-r border-gray-800 flex flex-col py-2 shrink-0">
            <button
              onClick={() => setActiveTab('agents')}
              className={`px-4 py-2 text-sm text-left transition ${activeTab === 'agents' ? 'text-blue-400 bg-blue-500/10 font-medium border-r-2 border-blue-500' : 'text-gray-400 hover:text-gray-200'}`}
            >
              Agents & Models
            </button>
            <button
              onClick={() => setActiveTab('general')}
              className={`px-4 py-2 text-sm text-left transition ${activeTab === 'general' ? 'text-blue-400 bg-blue-500/10 font-medium border-r-2 border-blue-500' : 'text-gray-400 hover:text-gray-200'}`}
            >
              General
            </button>
          </div>

          {/* Content */}
          <div className="flex-1 overflow-y-auto p-5 bg-background">
            
            {activeTab === 'agents' && (
              <div className="space-y-6">
                <div className="flex items-center justify-between">
                  <h3 className="text-base font-semibold text-gray-300">Configured Agents</h3>
                  <div className="flex gap-2">
                    <button
                      onClick={handleAutodetect}
                      disabled={isDetecting}
                      className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-gray-300 bg-gray-800 hover:bg-gray-700 rounded-md border border-gray-700 transition disabled:opacity-50"
                    >
                      <Search className="w-3.5 h-3.5" />
                      {isDetecting ? 'Detecting...' : 'Auto-detect'}
                    </button>
                    <button
                      onClick={() => setShowAddForm(!showAddForm)}
                      className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-white bg-blue-600 hover:bg-blue-500 rounded-md transition"
                    >
                      <Plus className="w-3.5 h-3.5" />
                      Add Custom
                    </button>
                  </div>
                </div>

                {showAddForm && (
                  <div className="p-4 bg-panel border border-blue-500/30 rounded-lg space-y-4">
                    <div className="grid grid-cols-2 gap-4">
                      <div>
                        <label className="block text-xs text-gray-400 mb-1">ID (e.g., custom-agent)</label>
                        <input
                          type="text"
                          value={newAgent.id || ''}
                          onChange={e => setNewAgent({...newAgent, id: e.target.value})}
                          className="w-full bg-background border border-gray-700 rounded-md px-3 py-1.5 text-sm"
                        />
                      </div>
                      <div>
                        <label className="block text-xs text-gray-400 mb-1">Name (e.g., Custom CLI)</label>
                        <input
                          type="text"
                          value={newAgent.name || ''}
                          onChange={e => setNewAgent({...newAgent, name: e.target.value})}
                          className="w-full bg-background border border-gray-700 rounded-md px-3 py-1.5 text-sm"
                        />
                      </div>
                      <div className="col-span-2">
                        <label className="block text-xs text-gray-400 mb-1">Command executable</label>
                        <input
                          type="text"
                          value={newAgent.command || ''}
                          onChange={e => setNewAgent({...newAgent, command: e.target.value})}
                          placeholder="e.g. claude, codex, or /absolute/path/to/bin"
                          className="w-full bg-background border border-gray-700 rounded-md px-3 py-1.5 text-sm"
                        />
                      </div>
                    </div>

                    <div className="border-t border-gray-800 pt-3">
                      <label className="block text-xs text-gray-400 mb-2">Models</label>
                      <div className="space-y-2 mb-2">
                        {newAgent.models?.map((m, i) => (
                          <div key={i} className="flex items-center gap-2 text-xs bg-background p-2 rounded border border-gray-800">
                            <Monitor className="w-3 h-3 text-gray-500" />
                            <span className="font-mono text-blue-400">{m.id}</span>
                            <span className="text-gray-400">({m.name})</span>
                          </div>
                        ))}
                      </div>
                      <div className="flex gap-2">
                        <input
                          type="text"
                          placeholder="Model ID"
                          value={newModel.id}
                          onChange={e => setNewModel({...newModel, id: e.target.value})}
                          className="flex-1 bg-background border border-gray-700 rounded-md px-3 py-1.5 text-sm"
                        />
                        <input
                          type="text"
                          placeholder="Model Name"
                          value={newModel.name}
                          onChange={e => setNewModel({...newModel, name: e.target.value})}
                          className="flex-1 bg-background border border-gray-700 rounded-md px-3 py-1.5 text-sm"
                        />
                        <button onClick={handleAddModel} className="px-3 py-1.5 bg-gray-800 hover:bg-gray-700 rounded-md text-xs">Add Model</button>
                      </div>
                    </div>
                    
                    <div className="flex justify-end pt-2">
                      <button onClick={handleAddAgent} className="px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white text-sm rounded-md font-medium">Save Agent</button>
                    </div>
                  </div>
                )}

                <div className="space-y-3">
                  {agents.map(agent => (
                    <div key={agent.id} className="p-4 bg-panel border border-gray-800 rounded-lg flex flex-col gap-3 group">
                      <div className="flex items-start justify-between">
                        <div>
                          <h4 className="font-semibold text-gray-200">{agent.name}</h4>
                          <div className="flex items-center gap-2 mt-1">
                            <span className="text-xs font-mono text-gray-500 bg-gray-900 px-1.5 py-0.5 rounded border border-gray-800">{agent.command}</span>
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
                          className="p-1.5 text-gray-500 hover:text-red-400 hover:bg-red-500/10 rounded-md transition opacity-0 group-hover:opacity-100"
                          title="Delete Agent"
                        >
                          <Trash2 className="w-4 h-4" />
                        </button>
                      </div>
                      
                      <div className="flex flex-wrap gap-2">
                        {agent.models.map(m => (
                          <span key={m.id} className="text-xs px-2 py-1 bg-background border border-gray-800 text-gray-400 rounded-md">
                            {m.name}
                          </span>
                        ))}
                      </div>
                    </div>
                  ))}
                  {agents.length === 0 && (
                    <div className="text-center p-8 border border-dashed border-gray-700 rounded-lg text-gray-500 text-sm">
                      No agents configured. Click Auto-detect to search for installed agents.
                    </div>
                  )}
                </div>
              </div>
            )}

            {activeTab === 'general' && (
              <div className="space-y-6">
                <h3 className="text-base font-semibold text-gray-300">General Settings</h3>
                <p className="text-sm text-gray-500">More settings coming soon.</p>
              </div>
            )}

          </div>
        </div>
      </div>
    </div>
  )
}
