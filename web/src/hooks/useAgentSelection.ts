import { useState } from 'react'
import { useLocalStorage } from '@/hooks/useLocalStorage'
import { pickDefaultModelId, pushRecentModel } from '@/lib/modelPrefs'
import type { Agent, Session } from '@/types'

/** Options passed into {@link useAgentSelection}. */
interface UseAgentSelectionOptions {
  agents: Agent[]
  sessions: Session[]
  activeSessionId: string | null
  hasConversation: boolean
  onRebindSession: (
    sessionId: string,
    agentId: string,
    modelId: string,
    maxTransferBytes?: number,
  ) => void
}

/** Return value of {@link useAgentSelection}. */
interface UseAgentSelectionResult {
  selectedAgent: string
  selectedModel: string
  effectiveAgentId: string
  effectiveModelId: string
  currentAgent: Agent | undefined
  userSelectedModel: string | null
  activeSession: Session | undefined
  pendingAgentId: string | null
  truncateLength: number
  setTruncateLength: (n: number) => void
  rebinding: boolean
  handleAgentChange: (agentId: string) => void
  confirmSwitchAgent: () => void
  cancelSwitchAgent: () => void
  handleModelChange: (modelId: string) => void
}

/**
 * Owns agent/model selection state and handlers extracted from ChatPanel.
 *
 * Persisted agent/model selections are restored from localStorage on mount via
 * {@link useLocalStorage}, falling back to the first available agent/model when
 * the stored value is missing or no longer valid (UI Spec §6.2 — UI
 * Persistence). The `selected*` values below are derived from the stored values
 * against the current agents list so a stale stored selection (agent/model
 * removed since last run) is corrected reactively as agents load asynchronously
 * — no separate prevAgents reconciliation block.
 *
 * For an active session the session's agent/model takes priority so switching
 * reflects that conversation. For a new chat (no active session) the local
 * selection drives the choice. The locally-selected model takes priority over
 * the session's persisted model so the dropdown visually updates immediately
 * when the user picks a different model — the backend switch is deferred to
 * send time. The guard `currentAgent?.models.some` ensures the stored model
 * belongs to the effective agent, so switching to a session with a different
 * agent doesn't show a stale model from the old one.
 */
export function useAgentSelection({
  agents,
  sessions,
  activeSessionId,
  hasConversation,
  onRebindSession,
}: UseAgentSelectionOptions): UseAgentSelectionResult {
  const [storedAgent, setStoredAgent] = useLocalStorage<string>('lai:selectedAgent', '')
  const [storedModel, setStoredModel] = useLocalStorage<string>('lai:selectedModel', '')
  const selectedAgent = agents.some((a) => a.id === storedAgent)
    ? storedAgent
    : (agents[0]?.id ?? '')
  const agentForModel = agents.find((a) => a.id === selectedAgent)
  // Prefer stored selection; fall back to agent-preferred (e.g. Devin currentValue), then first.
  const selectedModel = pickDefaultModelId(agentForModel?.models ?? [], storedModel)

  // Switch-agent confirmation dialog state. When the user changes the agent
  // dropdown mid-conversation, we show a dialog instead of rebinding
  // immediately so they can pick a transfer-history truncate length.
  const [pendingAgentId, setPendingAgentId] = useState<string | null>(null)
  const [truncateLength, setTruncateLength] = useState<number>(8000)
  // True while a switch-agent rebind is in flight — disables the dialog
  // buttons so a double-click can't fire onRebindSession twice.
  const [rebinding, setRebinding] = useState(false)

  const activeSession = sessions.find((s) => s.id === activeSessionId)
  const effectiveAgentId = activeSession?.agentId || selectedAgent || agents[0]?.id || ''
  const currentAgent = agents.find((a) => a.id === effectiveAgentId)
  const userSelectedModel =
    selectedModel && currentAgent?.models.some((m) => m.id === selectedModel)
      ? selectedModel
      : null
  const effectiveModelId =
    userSelectedModel || activeSession?.modelId || currentAgent?.models[0]?.id || ''

  /** Updates models when the harness changes; rebinds an active conversation. */
  const handleAgentChange = (agentId: string) => {
    const previousAgentId = effectiveAgentId
    if (!hasConversation) {
      setStoredAgent(agentId)
      const agent = agents.find((a) => a.id === agentId)
      const modelId = pickDefaultModelId(agent?.models ?? [], '')
      if (agent) setStoredModel(modelId)
      if (activeSessionId && modelId) onRebindSession(activeSessionId, agentId, modelId)
      return
    }
    if (agentId === previousAgentId) return
    setPendingAgentId(agentId)
  }

  /** Confirms the pending agent switch and rebinds with the chosen truncate length. */
  const confirmSwitchAgent = () => {
    if (!pendingAgentId || !activeSessionId || rebinding) {
      setPendingAgentId(null)
      return
    }
    const agentId = pendingAgentId
    setStoredAgent(agentId)
    const agent = agents.find((a) => a.id === agentId)
    const modelId = pickDefaultModelId(agent?.models ?? [], '')
    if (agent) setStoredModel(modelId)
    const maxBytes = truncateLength > 0 ? truncateLength : undefined
    setRebinding(true)
    try {
      onRebindSession(activeSessionId, agentId, modelId, maxBytes)
    } finally {
      setRebinding(false)
      setPendingAgentId(null)
    }
  }

  /** Cancels the pending agent switch — reverts the dropdown to the current agent. */
  const cancelSwitchAgent = () => {
    setPendingAgentId(null)
  }

  /** Updates the locally-selected model only. The backend switch is deferred
   *  to send time (see handleSend) so the user can change their mind in the
   *  dropdown without round-tripping to the server on every selection — only
   *  the model that's actually in effect when a prompt is sent gets applied.
   *  Also records the pick in per-agent recent history for the dropdown. */
  const handleModelChange = (modelId: string) => {
    setStoredModel(modelId)
    if (effectiveAgentId) pushRecentModel(effectiveAgentId, modelId)
  }

  return {
    selectedAgent,
    selectedModel,
    effectiveAgentId,
    effectiveModelId,
    currentAgent,
    userSelectedModel,
    activeSession,
    pendingAgentId,
    truncateLength,
    setTruncateLength,
    rebinding,
    handleAgentChange,
    confirmSwitchAgent,
    cancelSwitchAgent,
    handleModelChange,
  }
}
