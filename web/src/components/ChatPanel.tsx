import { useEffect, useState, useRef, useMemo, type ChangeEvent, type CSSProperties } from 'react'
import { WifiOff } from 'lucide-react'
import { cn } from '@/lib/utils'
import {
  api,
  getProfiles,
  setSessionProfile,
  type SessionHistoryCapabilities,
  type UploadResult,
  type ProfileConfig,
} from '@/lib/api'
import { isSessionNotFound } from '@/lib/errors'
import { ChatTabBar } from './ChatTabBar'
import { ChatComposer } from './ChatComposer'
import { ConversationView } from './ConversationView'
import { ChatHistory } from './ChatHistory'
import { SwitchAgentDialog } from './SwitchAgentDialog'
import { WorkspaceBar } from './chat/WorkspaceBar'
import { Banner } from './ui/Banner'
import { useAutoscroll } from '@/hooks/useAutoscroll'
import { useChatTabs } from '@/hooks/useChatTabs'
import { useLocalStorage } from '@/hooks/useLocalStorage'
import { useMcpServers } from '@/hooks/useMcpServers'
import { pickDefaultModelId, pushRecentModel } from '@/lib/modelPrefs'
import type { AppEvent, Agent, Attachment, Session } from '@/types'
import type { PendingPermission } from '@/lib/api'

export interface ChatPanelActions {
  onSendMessage: (sessionId: string, content: string, attachments?: Attachment[]) => Promise<void>
  onCreateSession: (agentId: string, modelId: string, profileId?: string) => Promise<string>
  onPermissionResponse: (requestId: string, decision: string) => void
  onSelectSession: (sessionId: string) => void
  onCancel: (sessionId: string) => void
  onRenameSession: (sessionId: string, name: string) => void
  onDeleteSession: (sessionId: string) => void
  onRebindSession: (sessionId: string, agentId: string, modelId: string, maxTransferBytes?: number) => void
  /** Switches the model on a live session without restarting the agent process
   *  (preserves conversation history). Used by handleModelChange for model-only
   *  switches; onRebindSession is still used for agent (harness) switches. */
  onSwitchModel: (sessionId: string, modelId: string) => Promise<void>
  onExportSession: (sessionId: string) => void
  /** Uploads a file to a session's upload store. Routed through useBackend so
   *  uploads share the hook's session-recovery semantics instead of bypassing
   *  it via api.uploadFile directly. */
  onUploadFile: (sessionId: string, file: File) => Promise<UploadResult>
  /** Workspace change handler. */
  onSelectWorkspace: (id: string) => void
}

/**
 * Right sidebar — agent chat (Blueprint Sec 17 — right sidebar).
 *
 * Slim orchestrator after the WI-3 restructure: owns agent/model state +
 * persistence, the switch-agent dialog, autoscroll, history popout open
 * state, input/attachment/sending/uploading state, and the open-tab ids.
 * Header markup lives in `ChatTabBar`, message rendering in `ConversationView`,
 * input markup in `ChatComposer`.
 *
 * On desktop: always visible alongside editor.
 * On mobile: full-screen via bottom nav.
 */
export function ChatPanel({
  events,
  allEvents,
  agents,
  sessions,
  workspaces,
  visible,
  connected,
  isDesktop,
  pendingPermissions,
  activeSessionId,
  actions,
  workspaceId,
  style,
  onOpenMcpSettings,
}: {
  events: AppEvent[]
  /** All events across all sessions — used only to compute the running
   *  indicator for non-active open tabs. The active session's running state
   *  is derived from `events` (already filtered to it) plus the `sending`
   *  state. Don't use this for conversation rendering. */
  allEvents: AppEvent[]
  agents: Agent[]
  sessions: Session[]
  workspaces: { id: string; name: string }[]
  visible: boolean
  connected: boolean
  isDesktop: boolean
  pendingPermissions: PendingPermission[]
  activeSessionId: string | null
  actions: ChatPanelActions
  /** Active workspace id. */
  workspaceId: string
  /** Optional inline style — used by App.tsx to apply a persisted panel width on desktop. */
  style?: CSSProperties
  /**
   * Opens app Settings focused on the MCP Servers section. Forwarded from
   * App → ChatComposer → McpPopout Settings icon.
   */
  onOpenMcpSettings?: () => void
}) {
  const {
    onSendMessage,
    onCreateSession,
    onPermissionResponse,
    onSelectSession,
    onCancel,
    onRenameSession,
    onDeleteSession,
    onRebindSession,
    onSwitchModel,
    onExportSession,
    onUploadFile,
    onSelectWorkspace,
  } = actions

  // Persisted agent/model selections and open-tab ids — restored from
  // localStorage on mount via useLocalStorage, falling back to the first
  // available agent/model when the stored value is missing or no longer valid
  // (UI Spec §6.2 — UI Persistence). The selected* values below are derived
  // from the stored values against the current agents list so a stale stored
  // selection (agent/model removed since last run) is corrected reactively as
  // agents load asynchronously — no separate prevAgents reconciliation block.
  const [storedAgent, setStoredAgent] = useLocalStorage<string>('lai:selectedAgent', '')
  const [storedModel, setStoredModel] = useLocalStorage<string>('lai:selectedModel', '')
  const selectedAgent = agents.some((a) => a.id === storedAgent)
    ? storedAgent
    : (agents[0]?.id ?? '')
  const agentForModel = agents.find((a) => a.id === selectedAgent)
  // Prefer stored selection; fall back to agent-preferred (e.g. Devin currentValue), then first.
  const selectedModel = pickDefaultModelId(agentForModel?.models ?? [], storedModel)

  const [chatHistoryOpen, setChatHistoryOpen] = useState(false)
  const [historyCapabilityState, setHistoryCapabilityState] = useState<{
    sessionId: string
    caps?: SessionHistoryCapabilities
  }>()
  const [input, setInput] = useState('')
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Transient "New chat" placeholder tab. Set true by handleNewChat so the tab
  // bar shows a "New chat" tab immediately (without a backend round-trip).
  // Cleared once a real session becomes active (via send/upload) or another
  // tab is selected. This keeps new-tab creation instant — the actual session
  // is created lazily on first send/upload by handleSend/handleFileChange.
  const [showNewChatTab, setShowNewChatTab] = useState(false)

  // Pending image attachments for the next prompt. `pendingAttachments` holds
  // the {id, name, mimeType} sent with the prompt; `pendingPreviews` holds the
  // {url, name} used to render thumbnails before the message is sent.
  const [pendingAttachments, setPendingAttachments] = useState<Attachment[]>([])
  const [pendingPreviews, setPendingPreviews] = useState<{ url: string; name: string }[]>([])
  const [uploading, setUploading] = useState(false)
  const [uploadError, setUploadError] = useState<string | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  // Switch-agent confirmation dialog state. When the user changes the agent
  // dropdown mid-conversation, we show a dialog instead of rebinding
  // immediately so they can pick a transfer-history truncate length.
  const [pendingAgentId, setPendingAgentId] = useState<string | null>(null)
  const [truncateLength, setTruncateLength] = useState<number>(8000)

  const {
    mcpServers,
    mcpHealth,
    mcpStatusLoading,
    mcpTogglingServer,
    mcpConfigChanged,
    setMcpConfigChanged,
    loadMcpStatus,
    handleToggleMcpServer,
  } = useMcpServers()

  // Profile config + per-session selection. Profiles are fetched on mount
  // and refreshed when Settings dispatches `profiles-changed`. The selected
  // profile id is derived during render from (a) a session-scoped user
  // override, (b) the value persisted in localStorage for the active session,
  // or (c) the configured defaultProfileId. Deriving during render avoids
  // setState-in-effect cascading renders while still restoring the per-session
  // profile on session switch. The selection is pushed to the backend via
  // POST /sessions/:id/profile on change so the next prompt uses it.
  const [profileConfig, setProfileConfig] = useState<ProfileConfig | null>(null)
  const profiles = useMemo(
    () =>
      profileConfig
        ? Object.entries(profileConfig.profiles).map(([id, entry]) => ({
            id,
            label: entry.label,
          }))
        : [],
    [profileConfig],
  )

  // Session-scoped user override — set by handleProfileChange so the dropdown
  // updates immediately on click, before/after the backend round-trip. Scoped
  // by sessionId so switching sessions ignores the prior session's override
  // and falls back to the persisted/default value for the new session.
  const [profileOverride, setProfileOverride] = useState<{
    sessionId: string
    profileId: string
  } | null>(null)

  const selectedProfileId = useMemo(() => {
    // '' override is the pre-session (new chat) pick; activeSessionId is null then.
    const overrideApplies =
      !!profileOverride &&
      (profileOverride.sessionId === activeSessionId ||
        (profileOverride.sessionId === '' && !activeSessionId))

    let resolved: string
    if (overrideApplies && profileOverride) {
      resolved = profileOverride.profileId
    } else if (!profileConfig) {
      return ''
    } else if (!activeSessionId) {
      resolved = profileConfig.defaultProfileId
    } else {
      try {
        const persisted = localStorage.getItem(`local-agent:profile:${activeSessionId}`)
        resolved = persisted || profileConfig.defaultProfileId
      } catch {
        // localStorage unavailable (private mode / disabled) — fall back to default.
        resolved = profileConfig.defaultProfileId
      }
    }

    // Drop deleted/unknown ids so the composer never renders a raw orphan id.
    if (profileConfig && resolved && !profileConfig.profiles[resolved]) {
      if (activeSessionId) {
        try {
          localStorage.removeItem(`local-agent:profile:${activeSessionId}`)
        } catch {
          // Ignore storage failures — fallback below still keeps the UI valid.
        }
      }
      return profileConfig.defaultProfileId
    }
    return resolved
  }, [profileOverride, activeSessionId, profileConfig])

  // Fetch profile config on mount and whenever Settings saves profiles.
  // Errors are non-fatal — the selector stays empty until a successful reload.
  useEffect(() => {
    let cancelled = false
    const loadProfiles = () => {
      void getProfiles()
        .then((cfg) => {
          if (cancelled) return
          setProfileConfig(cfg)
        })
        .catch((err) => {
          // Log but don't surface — the composer degrades to an empty dropdown.
          console.error('Failed to load chat profiles:', err)
        })
    }
    loadProfiles()
    const onProfilesChanged = () => {
      loadProfiles()
    }
    window.addEventListener('profiles-changed', onProfilesChanged)
    return () => {
      cancelled = true
      window.removeEventListener('profiles-changed', onProfilesChanged)
    }
  }, [])

  /** Switches the active session's profile: pushes the new id to the backend
   *  (POST /sessions/:id/profile), then persists it in localStorage. The
   *  dropdown updates immediately via `profileOverride` (session-scoped so
   *  switching sessions restores the other session's profile). On error,
   *  reverts the override and surfaces an inline message via `error`. */
  const handleProfileChange = async (profileId: string) => {
    if (profileId === selectedProfileId) return
    if (!activeSessionId) {
      // No live session yet — remember the pick; handleSend applies it after
      // onCreateSession with profileId before ACP actor startup.
      setProfileOverride({ sessionId: '', profileId })
      return
    }
    const previous = selectedProfileId
    setProfileOverride({ sessionId: activeSessionId, profileId })
    try {
      await setSessionProfile(activeSessionId, profileId)
      try {
        localStorage.setItem(`local-agent:profile:${activeSessionId}`, profileId)
      } catch {
        // Ignore write failures (quota / disabled storage) — selection still applies for this session.
      }
    } catch (err) {
      // Revert the dropdown and surface the error so the user knows the
      // backend switch failed (e.g. unknown profile id, session gone).
      setProfileOverride({ sessionId: activeSessionId, profileId: previous })
      const message = err instanceof Error ? err.message : 'Failed to switch profile'
      if (isSessionNotFound(message)) {
        setError('This conversation is no longer available. Start a new chat.')
        onSelectSession('')
      } else {
        setError(message)
      }
    }
  }

  /** Restart for MCP: ACP doesn't support live add/remove in v1, so the
   *  simplest correct action is to drop the active session and let the user
   *  start a new chat, which will pick up the updated MCP config. */
  const handleRestartForMcp = () => {
    setMcpConfigChanged(false)
    onSelectSession('')
    setShowNewChatTab(true)
  }

  // Scroll container ref for the smart-autoscroll hook (Feature 1).
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const { openTabIds, openTab, handleCloseTab } = useChatTabs({
    sessions,
    activeSessionId,
    onSelectSession,
  })

  // The active conversation owns its agent/model — derive the selectors from it
  // so switching reflects that conversation. For a new chat (no active session)
  // the local selection drives the choice. The locally-selected model takes
  // priority over the session's persisted model so the dropdown visually updates
  // immediately when the user picks a different model — the backend switch is
  // deferred to send time (see handleSend). The guard `currentAgent?.models.some`
  // ensures the stored model belongs to the effective agent, so switching to a
  // session with a different agent doesn't show a stale model from the old one.
  const activeSession = sessions.find((s) => s.id === activeSessionId)
  useEffect(() => {
    let cancelled = false
    if (!activeSessionId || !chatHistoryOpen) {
      return () => { cancelled = true }
    }
    void api.getSessionHistoryCapabilities(activeSessionId)
      .then((caps) => {
        if (!cancelled) setHistoryCapabilityState({ sessionId: activeSessionId, caps })
      })
      .catch(() => {
        if (!cancelled) setHistoryCapabilityState({ sessionId: activeSessionId })
      })
    return () => { cancelled = true }
  }, [activeSessionId, chatHistoryOpen])
  const effectiveAgentId = activeSession?.agentId || selectedAgent || agents[0]?.id || ''
  const currentAgent = agents.find((a) => a.id === effectiveAgentId)
  const userSelectedModel =
    selectedModel && currentAgent?.models.some((m) => m.id === selectedModel)
      ? selectedModel
      : null
  const effectiveModelId =
    userSelectedModel || activeSession?.modelId || currentAgent?.models[0]?.id || ''

  // Determine whether a turn is currently in flight, to toggle Send/Stop.
  const lastEvent = events[events.length - 1]
  const agentRunning =
    sending ||
    (!!lastEvent &&
      ((lastEvent.type === 'StreamUpdate' && !!lastEvent.streaming) ||
        lastEvent.type === 'ResponseStarted' ||
        lastEvent.type === 'ToolStarted' ||
        lastEvent.type === 'ShellCommandStarted'))

  // Per-tab running indicator. The active session uses the global `sending`
  // state (only one send can be in flight at a time and it always targets the
  // active session) plus its last event. Non-active open tabs are derived
  // purely from their last event in `allEvents` — `sending` is NOT applied to
  // them, since a background tab's send state is owned by whichever session
  // was active when it was dispatched.
  const runningSessionIds = useMemo(() => {
    const ids = new Set<string>()
    const isRunningEvent = (e: AppEvent | undefined): boolean =>
      !!e &&
      ((e.type === 'StreamUpdate' && !!e.streaming) ||
        e.type === 'ResponseStarted' ||
        e.type === 'ToolStarted' ||
        e.type === 'ShellCommandStarted')
    if (activeSessionId && (sending || isRunningEvent(lastEvent))) {
      ids.add(activeSessionId)
    }
    for (const id of openTabIds) {
      if (id === activeSessionId) continue
      const eventsForTab = allEvents.filter((e) => e.sessionId === id)
      if (isRunningEvent(eventsForTab[eventsForTab.length - 1])) {
        ids.add(id)
      }
    }
    return ids
  }, [activeSessionId, sending, lastEvent, openTabIds, allEvents])

  // Tabs to render — sessions whose id is in openTabIds, in openTabIds order.
  const openTabs = openTabIds
    .map((id) => sessions.find((s) => s.id === id))
    .filter((s): s is Session => !!s)

  const hasConversation =
    !!activeSessionId && events.some((e) => e.type === 'PromptSubmitted')

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
    if (!pendingAgentId || !activeSessionId) {
      setPendingAgentId(null)
      return
    }
    const agentId = pendingAgentId
    setStoredAgent(agentId)
    const agent = agents.find((a) => a.id === agentId)
    const modelId = pickDefaultModelId(agent?.models ?? [], '')
    if (agent) setStoredModel(modelId)
    const maxBytes = truncateLength > 0 ? truncateLength : undefined
    onRebindSession(activeSessionId, agentId, modelId, maxBytes)
    setPendingAgentId(null)
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

  const handleSend = async () => {
    const content = input.trim()
    if ((!content && pendingAttachments.length === 0) || sending || uploading || !effectiveAgentId || !effectiveModelId) return

    setSending(true)
    setError(null)
    setInput('')

    try {
      let sessionId = activeSessionId
      const wasNewSession = !sessionId
      if (!sessionId) {
        sessionId = await onCreateSession(effectiveAgentId, effectiveModelId, selectedProfileId)
        // A real session now exists — drop the transient "New chat" placeholder.
        setShowNewChatTab(false)
        // The selected profile was supplied to session creation before actor
        // startup, including its MCP server allowlist.
        if (profileOverride?.sessionId === '') {
          const pendingProfileId = profileOverride.profileId
          setProfileOverride({ sessionId, profileId: pendingProfileId })
          try {
            localStorage.setItem(`local-agent:profile:${sessionId}`, pendingProfileId)
          } catch {
            // Ignore write failures (quota / disabled storage).
          }
        }
      }
      // Always ensure the active session is opened as a tab — covers both the
      // newly-created case above and an existing activeSessionId that hasn't
      // been added to openTabIds yet (the auto-open effect also handles this,
      // but calling it here makes the tab appear immediately on send).
      openTab(sessionId)
      // Deferred model switch: handleModelChange only updates local state, so
      // if the user picked a model that differs from an existing session's
      // current model, apply the switch on the backend now so this prompt uses
      // it. Skipped for newly-created sessions — profileId was supplied before
      // the ACP actor started, so its MCP server list is already correct.
      // the selected model, so switching again would be redundant.
      if (!wasNewSession && userSelectedModel && activeSession?.modelId !== userSelectedModel) {
        await onSwitchModel(sessionId, userSelectedModel)
      }
      const attachmentsToSend = pendingAttachments
      await onSendMessage(
        sessionId,
        content,
        attachmentsToSend.length > 0 ? attachmentsToSend : undefined,
      )
      // Successful send counts as use for the Recent dropdown section.
      if (effectiveAgentId && effectiveModelId) {
        pushRecentModel(effectiveAgentId, effectiveModelId)
      }
      setPendingAttachments([])
      setPendingPreviews([])
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to send message'
      if (isSessionNotFound(message)) {
        setError('This conversation is no longer available. Start a new chat.')
        onSelectSession('')
      } else {
        setError(message)
        setInput(content)
      }
    } finally {
      setSending(false)
    }
  }

  const handleStop = () => {
    if (activeSessionId) onCancel(activeSessionId)
    setSending(false)
  }

  /** Opens the native file picker for image attachments. */
  const handlePickFiles = () => {
    setUploadError(null)
    fileInputRef.current?.click()
  }

  /** Uploads each selected image and appends it to the pending attachments.
   *  Creates a session on demand if the user is in the "new chat" state, since
   *  uploads require an existing session id. */
  const handleFileChange = async (e: ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files || files.length === 0) return
    e.target.value = ''

    if (!effectiveAgentId || !effectiveModelId) return
    setUploading(true)
    setUploadError(null)
    try {
      let sessionId = activeSessionId
      if (!sessionId) {
        sessionId = await onCreateSession(effectiveAgentId, effectiveModelId, selectedProfileId)
        // A real session now exists — drop the transient "New chat" placeholder.
        setShowNewChatTab(false)
      }
      // Always ensure the active session is opened as a tab (see handleSend).
      openTab(sessionId)
      for (const file of Array.from(files)) {
        const result = await onUploadFile(sessionId, file)
        setPendingAttachments((prev) => [
          ...prev,
          { id: result.id, name: result.name, mimeType: result.mimeType },
        ])
        setPendingPreviews((prev) => [...prev, { url: result.url, name: result.name }])
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to upload file'
      if (isSessionNotFound(message)) {
        setError('This conversation is no longer available. Start a new chat.')
        onSelectSession('')
      } else {
        setUploadError(message)
      }
    } finally {
      setUploading(false)
    }
  }

  /** Removes a pending attachment by index. */
  const removePendingAttachment = (index: number) => {
    setPendingAttachments((prev) => prev.filter((_, i) => i !== index))
    setPendingPreviews((prev) => prev.filter((_, i) => i !== index))
  }

  // Map permission request IDs to their resolution so resolved cards collapse.
  const permissionResolution = new Map<string, 'granted' | 'denied'>()
  for (const e of events) {
    if (e.requestId && (e.type === 'PermissionGranted' || e.type === 'PermissionDenied')) {
      permissionResolution.set(e.requestId, e.type === 'PermissionDenied' ? 'denied' : 'granted')
    }
  }

  const canSend = Boolean(
    (input.trim() || pendingAttachments.length > 0) &&
      effectiveAgentId &&
      effectiveModelId &&
      !sending &&
      !uploading,
  )

  /**
   * Merges consecutive StreamUpdate events into a single accumulated message
   * so streaming text appears as one growing response (like ChatGPT).
   * Non-stream events are passed through individually.
   */
  const mergedEvents: AppEvent[] = events.reduce((acc: AppEvent[], event: AppEvent) => {
    if (event.type === 'StreamUpdate') {
      const last = acc[acc.length - 1]
      if (last && last.type === 'StreamUpdate' && last.role === event.role && !!last.thought === !!event.thought) {
        acc[acc.length - 1] = {
          ...last,
          content: (last.content || '') + (event.content || ''),
          streaming: event.streaming,
        }
        return acc
      }
    }
    acc.push(event)
    return acc
  }, [])

  // Smart autoscroll — follows new content only when the user is already
  // near the bottom; otherwise stays put and shows a jump-to-bottom button.
  // `pendingPermissions` is in the deps because it arrives via a separate
  // async REST call (loadPendingPermissions) after the PermissionRequested
  // event — when it lands the permission card grows to show action buttons,
  // and we need to scroll again so the card isn't cut off at the bottom.
  const { isAtBottom, scrollToBottom } = useAutoscroll(
    scrollContainerRef,
    [mergedEvents, error, pendingPermissions],
  )

  const handleNewChat = () => {
    // Lazy session creation: just reset transient UI state and drop into the
    // new-chat state (activeSessionId = null). The actual backend session is
    // created on first send/upload by handleSend/handleFileChange, which both
    // already handle the null-activeSessionId case. This keeps "+" instant —
    // no agent-process spawn / ACP handshake round-trip on tab creation.
    // Show a transient "New chat" placeholder tab so the user sees feedback.
    setChatHistoryOpen(false)
    setError(null)
    setInput('')
    setShowNewChatTab(true)
    onSelectSession('')
  }

  /** Selecting a session from a tab or the history popout also opens it as a tab. */
  const handleSelectAndOpen = (id: string) => {
    if (id) {
      openTab(id)
      // Selecting a real session dismisses the transient "New chat" placeholder.
      setShowNewChatTab(false)
    }
    onSelectSession(id)
    setChatHistoryOpen(false)
  }

  return (
    <aside
      className={cn(
        'flex-col h-full shrink-0 w-full bg-background border-l border-border lg:w-96',
        visible ? 'flex' : 'hidden',
        'absolute inset-0 z-30 lg:relative lg:inset-auto lg:z-auto',
      )}
      style={style}
    >
      <ChatTabBar
        openTabs={openTabs}
        activeSessionId={activeSessionId}
        runningSessionIds={runningSessionIds}
        onSelectSession={handleSelectAndOpen}
        onNewChat={handleNewChat}
        onCloseTab={handleCloseTab}
        onToggleHistory={() => setChatHistoryOpen((v) => !v)}
        historyOpen={chatHistoryOpen}
        connected={connected}
        isDesktop={isDesktop}
        showNewChatTab={showNewChatTab}
        onCloseNewChatTab={() => {
          // Closing the transient "New chat" placeholder just drops it and
          // falls back to the most-recently still-open real tab (or the empty
          // new-chat state if none are open).
          setShowNewChatTab(false)
          if (!activeSessionId && openTabIds.length > 0) {
            onSelectSession(openTabIds[openTabIds.length - 1])
          }
        }}
        onExportSession={onExportSession}
      >
        <ChatHistory
          sessions={sessions}
          workspaces={workspaces}
          open={chatHistoryOpen}
          onClose={() => setChatHistoryOpen(false)}
          onCreateSession={handleNewChat}
          onSelectSession={handleSelectAndOpen}
          onRenameSession={onRenameSession}
          onExportSession={onExportSession}
          historyCapabilities={
            historyCapabilityState?.sessionId === activeSessionId
              ? historyCapabilityState.caps
              : undefined
          }
          onDeleteSession={(id) => {
            if (id === activeSessionId) onSelectSession('')
            onDeleteSession(id)
          }}
        />
      </ChatTabBar>

      {/* Disconnected banner — surfaces connection loss to the user. */}
      {!connected && (
        <Banner
          variant="warning"
          className="border-b px-3 py-2 flex items-center gap-2 shrink-0"
        >
          <WifiOff className="w-3.5 h-3.5" /> Reconnecting to daemon…
        </Banner>
      )}

      <ConversationView
        events={mergedEvents}
        pendingPermissions={pendingPermissions}
        permissionResolution={permissionResolution}
        onPermissionResponse={onPermissionResponse}
        error={error}
        scrollContainerRef={scrollContainerRef}
        isAtBottom={isAtBottom}
        onJumpToBottom={scrollToBottom}
        mcpConfigChanged={mcpConfigChanged}
        onDismissMcpBanner={() => setMcpConfigChanged(false)}
        onRestartForMcp={handleRestartForMcp}
      />

      <ChatComposer
        models={currentAgent?.models ?? []}
        agentId={effectiveAgentId}
        mcpServers={mcpServers}
        mcpStatusByName={mcpHealth}
        mcpStatusLoading={mcpStatusLoading}
        onToggleMcpServer={handleToggleMcpServer}
        mcpTogglingServer={mcpTogglingServer}
        onMcpPopoutOpen={loadMcpStatus}
        onOpenMcpSettings={onOpenMcpSettings}
        effectiveModelId={effectiveModelId}
        onModelChange={handleModelChange}
        input={input}
        onInputChange={setInput}
        onSend={handleSend}
        onStop={handleStop}
        agentRunning={agentRunning}
        canSend={canSend}
        pendingPreviews={pendingPreviews}
        onRemoveAttachment={removePendingAttachment}
        onPickFiles={handlePickFiles}
        uploading={uploading}
        uploadError={uploadError}
        disabled={sending || agents.length === 0}
        profiles={profiles}
        selectedProfileId={selectedProfileId}
        onProfileChange={handleProfileChange}
      />

      <WorkspaceBar
        agents={agents}
        currentAgentId={effectiveAgentId}
        onSelectAgent={handleAgentChange}
        workspaces={workspaces}
        workspaceId={workspaceId}
        onSelectWorkspace={onSelectWorkspace}
        workspaceDisabled={hasConversation}
        disabled={sending || agents.length === 0}
      />

      {/* Hidden file input — triggered by the attach button via ref. */}
      <input
        ref={fileInputRef}
        type="file"
        accept="image/png,image/jpeg,image/gif,image/webp"
        multiple
        onChange={handleFileChange}
        className="hidden"
      />

      {/* Switch-agent confirmation dialog — shown when the user changes the
          harness mid-conversation. Lets them pick how much of the prior
          conversation history to transfer as context for the new agent. */}
      <SwitchAgentDialog
        open={!!pendingAgentId}
        pendingAgentId={pendingAgentId}
        currentAgentName={currentAgent?.name ?? effectiveAgentId}
        pendingAgentName={agents.find((a) => a.id === pendingAgentId)?.name ?? ''}
        truncateLength={truncateLength}
        setTruncateLength={setTruncateLength}
        onConfirm={confirmSwitchAgent}
        onCancel={cancelSwitchAgent}
      />
    </aside>
  )
}
