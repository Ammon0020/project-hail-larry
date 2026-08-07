import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  getProfiles,
  previewSessionProfile,
  setSessionProfile,
  transitionSessionProfile,
  type ProfileConfig,
  type ProfileTransitionPreview,
} from '@/lib/api'
import type { ProfileTransitionChoice } from '@/components/ProfileTransitionDialog'
import { describeSessionError, SESSION_GONE_MESSAGE } from '@/lib/errors'
import { safeStorage } from '@/lib/safeStorage'

/** Options accepted by {@link useProfileSelection}. */
interface UseProfileSelectionOptions {
  /** Currently active chat session id, or `null` before a session is created. */
  activeSessionId: string | null
  /** Invoked when the active session disappears (e.g. backend 404 on profile switch). */
  onSelectSession: (sessionId: string) => void
  /** Surfaces inline error messages from profile-switch failures. */
  setError: (message: string | null) => void
  /**
   * Invoked with a session created outside the normal create flow (the `fresh`
   * transition strategy), so the app can refresh its session list and switch to
   * it. Without this the new conversation would exist on the server but be
   * invisible in the UI.
   */
  onSessionCreated?: (sessionId: string) => void
}

/** A selectable profile option surfaced to the composer dropdown. */
interface ProfileOption {
  id: string
  label: string
}

/** A profile switch waiting on the user's transition choice. */
export interface PendingProfileTransition {
  profileId: string
  profileLabel: string
  preview: ProfileTransitionPreview
}

/** Result returned by {@link useProfileSelection}. */
interface UseProfileSelectionResult {
  profiles: ProfileOption[]
  selectedProfileId: string
  profileOverride: { sessionId: string; profileId: string } | null
  setProfileOverride: (override: { sessionId: string; profileId: string } | null) => void
  handleProfileChange: (profileId: string) => Promise<void>
  /** Non-null while the transition dialog is open. */
  pendingTransition: PendingProfileTransition | null
  /** True while a chosen transition is in flight. */
  transitioning: boolean
  resolveTransition: (choice: ProfileTransitionChoice) => Promise<void>
  cancelTransition: () => void
  /**
   * Set when the session's instructions come from a profile whose MCP servers
   * it does not actually have — the "instructions only" outcome. Drives the
   * persistent disclosure so the selector never implies access it lacks.
   */
  instructionsOnlyNotice: string | null
}

/**
 * Owns profile config fetching and per-session profile selection state extracted
 * from `ChatPanel`.
 *
 * Profiles are fetched on mount and refreshed whenever Settings dispatches the
 * `profiles-changed` window event. The selected profile id is derived during
 * render from (a) a session-scoped user override, (b) the value persisted in
 * localStorage for the active session, or (c) the configured `defaultProfileId`.
 * Deriving during render avoids setState-in-effect cascading renders while still
 * restoring the per-session profile on session switch. The selection is pushed
 * to the backend via `POST /sessions/:id/profile` on change so the next prompt
 * uses it.
 *
 * @param options - Hook inputs (active session id + error/session callbacks).
 * @returns Profile list, derived selection id, override state, and change handler.
 */
export function useProfileSelection({
  activeSessionId,
  onSelectSession,
  setError,
  onSessionCreated,
}: UseProfileSelectionOptions): UseProfileSelectionResult {
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
        const persisted = safeStorage.get(`local-agent:profile:${activeSessionId}`)
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
          safeStorage.remove(`local-agent:profile:${activeSessionId}`)
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

  // A profile switch the user has not resolved yet — non-null keeps the
  // transition dialog open. The dropdown deliberately does NOT move until the
  // choice is committed, so a cancel leaves no trace.
  const [pendingTransition, setPendingTransition] = useState<PendingProfileTransition | null>(
    null,
  )
  const [transitioning, setTransitioning] = useState(false)

  // Cached notice tagged with the session+profile it was computed for. Deriving
  // the visible value during render (rather than clearing state in an effect)
  // means a stale notice can never be shown against a different session, and
  // keeps this hook free of setState-in-effect cascades.
  const [noticeState, setNoticeState] = useState<{ key: string; notice: string | null } | null>(
    null,
  )

  const profileLabel = useCallback(
    (profileId: string) => profileConfig?.profiles[profileId]?.label ?? profileId,
    [profileConfig],
  )

  /** Remember a session's profile locally; storage failures are non-fatal. */
  const persistProfile = useCallback((sessionId: string, profileId: string) => {
    safeStorage.set(`local-agent:profile:${sessionId}`, profileId)
  }, [])

  /** Surface a failure, treating a vanished session as a special case. */
  const reportFailure = useCallback(
    (err: unknown, fallback: string) => {
      const { message, sessionGone } = describeSessionError(err, fallback)
      if (sessionGone) {
        setError(SESSION_GONE_MESSAGE)
        onSelectSession('')
      } else {
        setError(message)
      }
    },
    [onSelectSession, setError],
  )

  /**
   * Switches the active session's profile.
   *
   * Asks the backend whether the target profile changes MCP server access
   * first. When it does not, this is the ordinary in-place switch. When it
   * does, the profile cannot be applied to the running agent session, so the
   * transition dialog opens and the dropdown stays put until the user chooses.
   */
  const handleProfileChange = async (profileId: string) => {
    if (profileId === selectedProfileId) return
    if (!activeSessionId) {
      // No live session yet — remember the pick; handleSend applies it after
      // onCreateSession with profileId before ACP actor startup.
      setProfileOverride({ sessionId: '', profileId })
      return
    }

    let preview
    try {
      preview = await previewSessionProfile(activeSessionId, profileId)
    } catch (err) {
      // Without a preview there is no way to know whether tool access would
      // change. Switching anyway could leave the selector claiming access the
      // session does not have, so surface the failure and change nothing.
      reportFailure(err, 'Failed to check profile tool access')
      return
    }

    if (preview.requiresNewSession) {
      setPendingTransition({ profileId, profileLabel: profileLabel(profileId), preview })
      return
    }

    const previous = selectedProfileId
    setProfileOverride({ sessionId: activeSessionId, profileId })
    try {
      await setSessionProfile(activeSessionId, profileId)
      persistProfile(activeSessionId, profileId)
    } catch (err) {
      // Revert the dropdown so it keeps matching the backend.
      setProfileOverride({ sessionId: activeSessionId, profileId: previous })
      reportFailure(err, 'Failed to switch profile')
    }
  }

  /** Applies the user's choice from the transition dialog. */
  const resolveTransition = async (choice: ProfileTransitionChoice) => {
    if (!pendingTransition || !activeSessionId) return
    const { profileId } = pendingTransition
    const previous = selectedProfileId
    setTransitioning(true)
    try {
      if (choice === 'instructions') {
        // Instructions move; MCP server access deliberately does not. The
        // persistent notice below keeps that visible.
        setProfileOverride({ sessionId: activeSessionId, profileId })
        await setSessionProfile(activeSessionId, profileId)
        persistProfile(activeSessionId, profileId)
      } else {
        const session = await transitionSessionProfile(activeSessionId, profileId, choice)
        persistProfile(session.id, profileId)
        setProfileOverride({ sessionId: session.id, profileId })
        if (choice === 'fresh') {
          // `history` returns the same session, already active. `fresh` creates
          // a separate conversation the app does not know about yet.
          onSessionCreated?.(session.id)
        }
      }
      setPendingTransition(null)
    } catch (err) {
      setProfileOverride({ sessionId: activeSessionId, profileId: previous })
      setPendingTransition(null)
      reportFailure(err, 'Failed to apply profile')
    } finally {
      setTransitioning(false)
    }
  }

  const cancelTransition = () => {
    setPendingTransition(null)
  }

  // Derive the instructions-only disclosure rather than storing it: the backend
  // preview already compares the servers the live agent session negotiated
  // against what the selected profile asks for. A difference means the session's
  // instructions and its tool access come from different profiles — which is
  // exactly the instructions-only outcome, and also what an edit to
  // profiles.json mid-session produces. Deriving keeps it correct across
  // reloads without persisting a flag that could go stale.
  const noticeKey =
    activeSessionId && selectedProfileId ? `${activeSessionId}:${selectedProfileId}` : ''
  const instructionsOnlyNotice =
    noticeKey && noticeState?.key === noticeKey ? noticeState.notice : null

  useEffect(() => {
    if (!noticeKey || !activeSessionId) return
    let cancelled = false
    void previewSessionProfile(activeSessionId, selectedProfileId)
      .then((preview) => {
        if (cancelled) return
        setNoticeState({
          key: noticeKey,
          notice: preview.requiresNewSession
            ? `Instructions: ${profileLabel(selectedProfileId)}; MCP servers: this session's existing access.`
            : null,
        })
      })
      .catch(() => {
        // Advisory only — a failed check must not block the composer. The
        // derived value stays null because no entry matches this key.
      })
    return () => {
      cancelled = true
    }
  }, [noticeKey, activeSessionId, selectedProfileId, profileLabel])

  return {
    profiles,
    selectedProfileId,
    profileOverride,
    setProfileOverride,
    handleProfileChange,
    pendingTransition,
    transitioning,
    resolveTransition,
    cancelTransition,
    instructionsOnlyNotice,
  }
}
