import { useEffect, useMemo, useState } from 'react'
import { getProfiles, setSessionProfile, type ProfileConfig } from '@/lib/api'
import { isSessionNotFound } from '@/lib/errors'

/** Options accepted by {@link useProfileSelection}. */
interface UseProfileSelectionOptions {
  /** Currently active chat session id, or `null` before a session is created. */
  activeSessionId: string | null
  /** Invoked when the active session disappears (e.g. backend 404 on profile switch). */
  onSelectSession: (sessionId: string) => void
  /** Surfaces inline error messages from profile-switch failures. */
  setError: (message: string | null) => void
}

/** A selectable profile option surfaced to the composer dropdown. */
interface ProfileOption {
  id: string
  label: string
}

/** Result returned by {@link useProfileSelection}. */
interface UseProfileSelectionResult {
  profiles: ProfileOption[]
  selectedProfileId: string
  profileOverride: { sessionId: string; profileId: string } | null
  setProfileOverride: (override: { sessionId: string; profileId: string } | null) => void
  handleProfileChange: (profileId: string) => Promise<void>
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

  return {
    profiles,
    selectedProfileId,
    profileOverride,
    setProfileOverride,
    handleProfileChange,
  }
}
