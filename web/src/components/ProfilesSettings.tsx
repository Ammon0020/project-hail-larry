import { useState, useEffect, useMemo, useCallback, useRef } from 'react'
import {
  Plus,
  Trash2,
  Save,
  RotateCcw,
  Check,
  AlertTriangle,
  Users,
} from 'lucide-react'
import {
  getMcpConfig,
  getProfiles,
  putProfiles,
  type ProfileConfig,
  type ProfileEntry,
} from '@/lib/api'
import { cn } from '@/lib/utils'

// Backend caps (S-PROF-REST). Mirrored here so we can validate inline before
// the round-trip and give immediate feedback — the backend re-validates and
// returns 400 with a message we surface inline on save.
const LABEL_MAX = 100
const INSTRUCTIONS_MAX = 16 * 1024 // 16 KiB
const ID_PATTERN = /^[a-zA-Z0-9_-]+$/

/** Empty profile entry used when adding a new profile. */
function emptyEntry(): ProfileEntry {
  return { label: '', instructions: '' }
}

/**
 * Deep-equality check for the profiles config. Used to drive the "unsaved
 * changes" indicator and to disable Save when there is nothing to persist.
 * Compares by value (not reference) since we clone on every edit.
 */
function configEqual(a: ProfileConfig, b: ProfileConfig): boolean {
  if (a.defaultProfileId !== b.defaultProfileId) return false
  const aKeys = Object.keys(a.profiles)
  const bKeys = Object.keys(b.profiles)
  if (aKeys.length !== bKeys.length) return false
  for (const k of aKeys) {
    const ap = a.profiles[k]
    const bp = b.profiles[k]
    if (!bp) return false
    if (ap.label !== bp.label) return false
    if (ap.instructions !== bp.instructions) return false
    if (ap.mcpServers === undefined || bp.mcpServers === undefined) {
      if (ap.mcpServers !== bp.mcpServers) return false
    } else if (ap.mcpServers.length !== bp.mcpServers.length) {
      return false
    } else {
      for (let i = 0; i < ap.mcpServers.length; i++) {
        if (ap.mcpServers[i] !== bp.mcpServers[i]) return false
      }
    }
  }
  return true
}

/** Parses the comma-separated MCP server text input into a normalized string[]. */
interface McpServerOption {
  name: string
  enabled: boolean
}

function parseMcpServerOptions(raw: string): McpServerOption[] {
  const parsed = JSON.parse(raw) as {
    mcpServers?: Record<string, { enabled?: boolean }>
  }
  return Object.entries(parsed.mcpServers ?? {})
    .map(([name, config]) => ({ name, enabled: config.enabled !== false }))
    .sort((a, b) => a.name.localeCompare(b.name))
}

/**
 * Settings → Profiles tab. Lists profiles from `GET /api/profiles`, lets the
 * user add / rename / edit instructions / set an MCP-server allowlist / delete, pick
 * the default profile, and persists the whole config via `PUT /api/profiles`.
 *
 * Backend validation errors (400) are surfaced inline next to Save — the
 * panel never claims success on failure and never silently reverts local
 * edits, so the user can fix and retry without losing their work.
 *
 * Available servers are loaded from MCP Settings. Existing selections that are
 * disabled or no longer configured stay visible so users can remove them.
 */
export function ProfilesSettings() {
  const [saved, setSaved] = useState<ProfileConfig | null>(null)
  const [draft, setDraft] = useState<ProfileConfig | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [mcpServers, setMcpServers] = useState<McpServerOption[]>([])
  const [mcpServersError, setMcpServersError] = useState<string | null>(null)
  const [savedFlash, setSavedFlash] = useState(false)
  const savedFlashTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const cfg = await getProfiles()
      setSaved(cfg)
      setDraft(cfg)
      // Keep a selection if it still exists, else fall back to the default.
      setSelectedId(prev =>
        prev && cfg.profiles[prev] ? prev : cfg.defaultProfileId,
      )
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  // Mirrors the loadMcp / loadProviders effects in SettingsPanel: the async
  // helper sets a 'loading' state before its first await (required for the
  // loading indicator). The set-state-in-effect rule flags interprocedural
  // calls through useCallback; same pattern, so we disable it here for parity.
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    load()
  }, [load])

  // Fetch the current MCP server options. Called on mount and whenever the
  // MCP config changes elsewhere in Settings (e.g. McpServersSettings saves or
  // toggles a server) via the 'mcp-changed' window event — mirrors the
  // 'profiles-changed' pattern used for profile reloads.
  const loadMcpServers = useCallback(async () => {
    try {
      const raw = await getMcpConfig()
      setMcpServers(parseMcpServerOptions(raw))
      setMcpServersError(null)
    } catch (err) {
      setMcpServers([])
      setMcpServersError(err instanceof Error ? err.message : String(err))
    }
  }, [])

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadMcpServers()
  }, [loadMcpServers])

  // Refresh the server list when MCP config changes in another settings
  // section (save/toggle) so the user doesn't have to reopen Settings.
  useEffect(() => {
    const onChange = () => { void loadMcpServers() }
    window.addEventListener('mcp-changed', onChange)
    return () => window.removeEventListener('mcp-changed', onChange)
  }, [loadMcpServers])

  // Clear the "Saved" flash timer if the component unmounts (e.g. the user
  // switches settings tabs) before the 2s timeout fires.
  useEffect(
    () => () => {
      if (savedFlashTimer.current) clearTimeout(savedFlashTimer.current)
    },
    [],
  )

  const dirty = useMemo(
    () => (saved && draft ? !configEqual(saved, draft) : false),
    [saved, draft],
  )

  /** Inline validation of the draft before sending. Returns a human-readable
   *  error string or null when valid. Mirrors the backend's checks so we can
   *  fail fast without a round-trip. */
  const inlineError = useMemo<string | null>(() => {
    if (!draft) return null
    const ids = Object.keys(draft.profiles)
    if (ids.length === 0) return 'At least one profile is required.'
    for (const id of ids) {
      if (!ID_PATTERN.test(id)) {
        return `Profile id "${id}" must match [a-zA-Z0-9_-]+.`
      }
      const p = draft.profiles[id]
      if (p.label.length > LABEL_MAX) {
        return `Profile "${id}" label exceeds ${LABEL_MAX} chars.`
      }
      if (p.instructions.length > INSTRUCTIONS_MAX) {
        return `Profile "${id}" instructions exceed ${INSTRUCTIONS_MAX} chars.`
      }
    }
    if (!draft.profiles[draft.defaultProfileId]) {
      return `defaultProfileId "${draft.defaultProfileId}" does not match any profile.`
    }
    return null
  }, [draft])

  const selected = selectedId && draft ? draft.profiles[selectedId] : null

  /** Generates a unique profile id like "profile-1", "profile-2", ... */
  function nextId(): string {
    const existing = new Set(draft ? Object.keys(draft.profiles) : [])
    let i = 1
    while (existing.has(`profile-${i}`)) i++
    return `profile-${i}`
  }

  function handleAdd() {
    if (!draft) return
    const id = nextId()
    const newEntry = emptyEntry()
    newEntry.label = `New Profile ${id}`
    setDraft({
      ...draft,
      profiles: { ...draft.profiles, [id]: newEntry },
    })
    setSelectedId(id)
  }

  function handleDelete(id: string) {
    if (!draft) return
    if (id === draft.defaultProfileId) {
      setError('Cannot delete the default profile. Pick another default first.')
      return
    }
    const nextProfiles = { ...draft.profiles }
    delete nextProfiles[id]
    const nextDraft = { ...draft, profiles: nextProfiles }
    setDraft(nextDraft)
    if (selectedId === id) {
      setSelectedId(nextDraft.defaultProfileId)
    }
    setError(null)
  }

  function updateSelected(patch: Partial<ProfileEntry>) {
    if (!draft || !selectedId || !draft.profiles[selectedId]) return
    setDraft({
      ...draft,
      profiles: {
        ...draft.profiles,
        [selectedId]: { ...draft.profiles[selectedId], ...patch },
      },
    })
  }

  function handleSetDefault(id: string) {
    if (!draft) return
    setDraft({ ...draft, defaultProfileId: id })
  }

  async function handleSave() {
    if (!draft) return
    if (inlineError) {
      setError(inlineError)
      return
    }
    setSaving(true)
    setError(null)
    try {
      await putProfiles(draft)
      setSaved(draft)
      setSavedFlash(true)
      savedFlashTimer.current = setTimeout(() => setSavedFlash(false), 2000)
      // Notify ChatPanel (and any other listeners) to re-fetch profile labels.
      window.dispatchEvent(new CustomEvent('profiles-changed'))
    } catch (e) {
      // Backend 400 carries an `error` body — surface it inline, do NOT
      // revert local edits so the user can fix and retry.
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  async function handleReset() {
    await load()
    setError(null)
  }

  if (loading) {
    return (
      <div className="space-y-4">
        <div className="flex items-center gap-2">
          <Users className="size-4 text-muted-foreground" />
          <h3 className="text-base font-semibold text-foreground">Profiles</h3>
        </div>
        <p className="text-xs text-muted-foreground">Loading…</p>
      </div>
    )
  }

  if (!draft) {
    return (
      <div className="space-y-4">
        <div className="flex items-center gap-2">
          <Users className="size-4 text-muted-foreground" />
          <h3 className="text-base font-semibold text-foreground">Profiles</h3>
        </div>
        <div className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive">
          <AlertTriangle className="mt-0.5 size-4 shrink-0" />
          <span className="font-mono break-all whitespace-pre-wrap">
            {error ?? 'Failed to load profiles.'}
          </span>
        </div>
        <button
          onClick={handleReset}
          className="flex items-center gap-2 rounded-md border border-input bg-secondary px-3 py-1.5 text-xs font-medium text-foreground transition hover:bg-accent"
        >
          <RotateCcw className="size-3.5" />
          Retry
        </button>
      </div>
    )
  }

  const profileIds = Object.keys(draft.profiles)

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Users className="size-4 text-muted-foreground" />
          <h3 className="text-base font-semibold text-foreground">Profiles</h3>
          {dirty && (
            <span className="flex items-center gap-1 text-[10px] text-amber-600 dark:text-amber-400">
              <span className="size-1.5 rounded-full bg-amber-500" />
              unsaved
            </span>
          )}
        </div>
        <button
          onClick={handleAdd}
          className="flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition hover:bg-primary/90"
        >
          <Plus className="size-3.5" />
          Add
        </button>
      </div>

      <p className="text-xs text-muted-foreground">
        Profiles bundle a label, system-prompt instructions, and complete MCP
        server access. MCP servers are selected when an ACP session starts.
      </p>

      <div className="flex flex-col gap-4 md:flex-row">
        {/* Profile list */}
        <div className="shrink-0 rounded-md border border-border bg-panel md:w-56">
          <ul className="max-h-72 overflow-y-auto md:max-h-112">
            {profileIds.length === 0 && (
              <li className="px-3 py-2 text-xs text-muted-foreground italic">
                No profiles.
              </li>
            )}
            {profileIds.map(id => {
              const p = draft.profiles[id]
              const active = id === selectedId
              const isDefault = id === draft.defaultProfileId
              return (
                <li key={id}>
                  <button
                    onClick={() => setSelectedId(id)}
                    aria-current={active ? 'true' : undefined}
                    className={cn(
                      'flex w-full items-center justify-between gap-2 px-3 py-2 text-left text-sm transition',
                      active
                        ? 'bg-primary/10 font-medium text-primary'
                        : 'text-foreground hover:bg-accent',
                    )}
                  >
                    <span className="truncate">
                      <span className="mr-1.5 font-mono text-xs text-muted-foreground">
                        {id}
                      </span>
                      <span className="truncate">{p.label || '(unnamed)'}</span>
                    </span>
                    {isDefault && (
                      <span className="shrink-0 rounded border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">
                        default
                      </span>
                    )}
                  </button>
                </li>
              )
            })}
          </ul>
        </div>

        {/* Editor */}
        <div className="min-w-0 flex-1">
          {selected && selectedId ? (
            <ProfileEditor
              id={selectedId}
              entry={selected}
              isDefault={draft.defaultProfileId === selectedId}
              labelTooLong={selected.label.length > LABEL_MAX}
              instructionsTooLong={selected.instructions.length > INSTRUCTIONS_MAX}
              mcpServers={mcpServers}
              mcpServersError={mcpServersError}
              onChange={updateSelected}
              onSetDefault={() => handleSetDefault(selectedId)}
              onDelete={() => handleDelete(selectedId)}
              canDelete={draft.defaultProfileId !== selectedId}
            />
          ) : (
            <div className="flex h-32 items-center justify-center rounded-md border border-dashed border-border text-xs text-muted-foreground italic">
              Select a profile to edit, or click Add.
            </div>
          )}
        </div>
      </div>

      {/* Default profile selector — full width below the list/editor. */}
      <div className="rounded-md border border-border bg-panel p-3">
        <label
          htmlFor="default-profile-select"
          className="mb-1 block text-xs text-muted-foreground"
        >
          Default profile
        </label>
        <select
          id="default-profile-select"
          value={draft.defaultProfileId}
          onChange={e => handleSetDefault(e.target.value)}
          className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-sm md:w-64"
        >
          {profileIds.map(id => (
            <option key={id} value={id}>
              {id} — {draft.profiles[id].label || '(unnamed)'}
            </option>
          ))}
        </select>
        <p className="mt-1 text-[11px] text-muted-foreground">
          Used when a chat session doesn't pick a profile explicitly.
        </p>
      </div>

      {/* Error / success banner + Save / Reset. */}
      {error && (
        <div className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive">
          <AlertTriangle className="mt-0.5 size-4 shrink-0" />
          <span className="font-mono break-all whitespace-pre-wrap">{error}</span>
        </div>
      )}

      <div className="flex items-center gap-2">
        <button
          onClick={handleSave}
          disabled={saving || !dirty || !!inlineError}
          className="flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition hover:bg-primary/90 disabled:opacity-50"
        >
          <Save className="size-3.5" />
          {saving ? 'Saving...' : 'Save'}
        </button>
        <button
          onClick={handleReset}
          disabled={saving || !dirty}
          className="flex items-center gap-2 rounded-md border border-input bg-secondary px-3 py-1.5 text-xs font-medium text-foreground transition hover:bg-accent disabled:opacity-50"
        >
          <RotateCcw className="size-3.5" />
          Reset
        </button>
        {savedFlash && (
          <span className="flex items-center gap-1 text-xs text-green-500">
            <Check className="size-3.5" />
            Saved
          </span>
        )}
      </div>
    </div>
  )
}

/**
 * Editor for a single profile entry. Controlled — parent owns the draft and
 * applies patches via `onChange`. Validation flags drive inline feedback
 * (red helper text under the offending field) so the user sees problems
 * before hitting Save.
 */
function ProfileEditor({
  id,
  entry,
  isDefault,
  labelTooLong,
  instructionsTooLong,
  mcpServers,
  mcpServersError,
  onChange,
  onSetDefault,
  onDelete,
  canDelete,
}: {
  id: string
  entry: ProfileEntry
  isDefault: boolean
  labelTooLong: boolean
  instructionsTooLong: boolean
  mcpServers: McpServerOption[]
  mcpServersError: string | null
  onChange: (patch: Partial<ProfileEntry>) => void
  onSetDefault: () => void
  onDelete: () => void
  canDelete: boolean
}) {
  const labelLen = entry.label.length
  const instrLen = entry.instructions.length
  // Choosing an explicit server policy completes the legacy tool-list
  // migration. `undefined` is intentional: JSON omits `legacyTools` on save.
  const updateServers = (mcpServers: string[] | undefined) =>
    onChange({ mcpServers, legacyTools: undefined })
  const selectedServers = new Set(entry.mcpServers ?? [])
  const knownServerNames = new Set(mcpServers.map(server => server.name))
  const disabledSelectedServers = mcpServers.filter(
    server => !server.enabled && selectedServers.has(server.name),
  )
  const staleServers = [...selectedServers].filter(name => !knownServerNames.has(name))
  const selectableServers = [
    ...mcpServers.filter(server => server.enabled),
    ...disabledSelectedServers,
    ...staleServers.map(name => ({ name, enabled: false })),
  ]

  const toggleServer = (name: string, checked: boolean) => {
    const next = new Set(entry.mcpServers ?? [])
    if (checked) next.add(name)
    else next.delete(name)
    updateServers([...next].sort())
  }

  return (
    <div className="space-y-4 rounded-lg border border-border bg-panel p-4">
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <span className="shrink-0 font-mono text-xs text-muted-foreground">
            {id}
          </span>
          {isDefault && (
            <span className="shrink-0 rounded border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">
              default
            </span>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {!isDefault && (
            <button
              onClick={onSetDefault}
              className="rounded-md border border-input bg-secondary px-2.5 py-1 text-xs font-medium text-foreground transition hover:bg-accent"
            >
              Set as default
            </button>
          )}
          <button
            onClick={onDelete}
            disabled={!canDelete}
            title={
              canDelete
                ? 'Delete this profile'
                : 'Cannot delete the default profile — pick another default first.'
            }
            className="flex items-center gap-1.5 rounded-md border border-destructive/30 bg-secondary px-2.5 py-1 text-xs font-medium text-destructive transition hover:bg-destructive/10 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <Trash2 className="size-3.5" />
            Delete
          </button>
        </div>
      </div>

      {/* Label */}
      <div>
        <div className="mb-1 flex items-baseline justify-between">
          <label
            htmlFor={`profile-label-${id}`}
            className="block text-xs text-muted-foreground"
          >
            Label
          </label>
          <span
            className={cn(
              'text-[10px] tabular-nums',
              labelTooLong ? 'text-destructive' : 'text-muted-foreground',
            )}
          >
            {labelLen}/{LABEL_MAX}
          </span>
        </div>
        <input
          id={`profile-label-${id}`}
          type="text"
          value={entry.label}
          onChange={e => onChange({ label: e.target.value })}
          maxLength={LABEL_MAX + 50}
          className={cn(
            'w-full rounded-md border bg-background px-3 py-1.5 text-sm',
            labelTooLong
              ? 'border-destructive focus:border-destructive'
              : 'border-input',
          )}
        />
        {labelTooLong && (
          <p className="mt-1 text-[11px] text-destructive">
            Label exceeds the {LABEL_MAX}-character cap.
          </p>
        )}
      </div>

      {/* Instructions */}
      <div>
        <div className="mb-1 flex items-baseline justify-between">
          <label
            htmlFor={`profile-instructions-${id}`}
            className="block text-xs text-muted-foreground"
          >
            Instructions
          </label>
          <span
            className={cn(
              'text-[10px] tabular-nums',
              instructionsTooLong ? 'text-destructive' : 'text-muted-foreground',
            )}
          >
            {instrLen.toLocaleString()}/{INSTRUCTIONS_MAX.toLocaleString()}
          </span>
        </div>
        <textarea
          id={`profile-instructions-${id}`}
          value={entry.instructions}
          onChange={e => onChange({ instructions: e.target.value })}
          rows={8}
          className={cn(
            'w-full resize-y rounded-md border bg-background px-3 py-2 font-mono text-sm',
            instructionsTooLong
              ? 'border-destructive focus:border-destructive'
              : 'border-input',
          )}
        />
        {instructionsTooLong && (
          <p className="mt-1 text-[11px] text-destructive">
            Instructions exceed the {INSTRUCTIONS_MAX.toLocaleString()}-character cap.
          </p>
        )}
      </div>

      {/* ACP attaches complete MCP servers at session startup; it cannot select
          individual tools from one server. */}
      <div>
        <label className="mb-2 flex items-center gap-2 text-xs text-foreground">
          <input
            type="checkbox"
            checked={entry.mcpServers === undefined}
            onChange={e => updateServers(e.target.checked ? undefined : [])}
          />
          All enabled MCP servers
        </label>
        <fieldset disabled={entry.mcpServers === undefined} className="space-y-1.5">
          <legend className="mb-1 text-xs text-muted-foreground">
            Selected MCP servers
          </legend>
          {mcpServersError ? (
            <p className="text-[11px] text-destructive">
              Could not load MCP servers: {mcpServersError}
            </p>
          ) : selectableServers.length === 0 ? (
            <p className="text-[11px] text-muted-foreground">
              No enabled MCP servers are configured.
            </p>
          ) : (
            <div className="max-h-40 space-y-1 overflow-y-auto rounded-md border border-input bg-background p-2">
              {selectableServers.map(server => (
                <label key={server.name} className="flex items-center gap-2 text-xs text-foreground">
                  <input
                    type="checkbox"
                    checked={selectedServers.has(server.name)}
                    disabled={!server.enabled && !selectedServers.has(server.name)}
                    onChange={event => toggleServer(server.name, event.target.checked)}
                  />
                  <span className="font-mono">{server.name}</span>
                  {!server.enabled && (
                    <span className="text-[10px] text-muted-foreground">unavailable</span>
                  )}
                </label>
              ))}
            </div>
          )}
        </fieldset>
        <p className="mt-1 text-[11px] text-muted-foreground">
          Leave “All enabled MCP servers” selected to omit this policy. Turn it
          off and select none to allow no MCP servers.
        </p>
        {entry.legacyTools && entry.legacyTools.length > 0 && (
          <p className="mt-2 text-[11px] text-amber-600 dark:text-amber-400">
            Legacy tool names ({entry.legacyTools.join(', ')}) were not converted
            to server names. Choose MCP servers above before saving this profile.
          </p>
        )}
      </div>
    </div>
  )
}
