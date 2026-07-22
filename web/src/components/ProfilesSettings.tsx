import { useState, useEffect, useMemo, useCallback } from 'react'
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
  return { label: '', instructions: '', tools: [] }
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
    if (ap.tools.length !== bp.tools.length) return false
    for (let i = 0; i < ap.tools.length; i++) {
      if (ap.tools[i] !== bp.tools[i]) return false
    }
  }
  return true
}

/** Parses the comma-separated tools text input into a normalized string[]. */
function parseTools(text: string): string[] {
  return text
    .split(',')
    .map(t => t.trim())
    .filter(Boolean)
}

/** Joins a tool whitelist back into the comma-separated editor string. */
function joinTools(tools: string[]): string {
  return tools.join(', ')
}

/**
 * Settings → Profiles tab. Lists profiles from `GET /api/profiles`, lets the
 * user add / rename / edit instructions / set a tool whitelist / delete, pick
 * the default profile, and persists the whole config via `PUT /api/profiles`.
 *
 * Backend validation errors (400) are surfaced inline next to Save — the
 * panel never claims success on failure and never silently reverts local
 * edits, so the user can fix and retry without losing their work.
 *
 * The tool whitelist is currently a comma-separated text input because the
 * MCP tool enumeration REST endpoint (S-PROF-TOOLS follow-up) is not yet
 * exposed. When it lands, swap the text input for per-server checkbox groups.
 */
export function ProfilesSettings() {
  const [saved, setSaved] = useState<ProfileConfig | null>(null)
  const [draft, setDraft] = useState<ProfileConfig | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [savedFlash, setSavedFlash] = useState(false)

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
      setTimeout(() => setSavedFlash(false), 2000)
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
          <Users className="w-4 h-4 text-muted-foreground" />
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
          <Users className="w-4 h-4 text-muted-foreground" />
          <h3 className="text-base font-semibold text-foreground">Profiles</h3>
        </div>
        <div className="flex items-start gap-2 p-3 text-xs text-destructive bg-destructive/10 border border-destructive/30 rounded-md">
          <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
          <span className="font-mono whitespace-pre-wrap break-all">
            {error ?? 'Failed to load profiles.'}
          </span>
        </div>
        <button
          onClick={handleReset}
          className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-foreground bg-secondary hover:bg-accent rounded-md border border-input transition"
        >
          <RotateCcw className="w-3.5 h-3.5" />
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
          <Users className="w-4 h-4 text-muted-foreground" />
          <h3 className="text-base font-semibold text-foreground">Profiles</h3>
          {dirty && (
            <span className="flex items-center gap-1 text-[10px] text-amber-600 dark:text-amber-400">
              <span className="w-1.5 h-1.5 rounded-full bg-amber-500" />
              unsaved
            </span>
          )}
        </div>
        <button
          onClick={handleAdd}
          className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition"
        >
          <Plus className="w-3.5 h-3.5" />
          Add
        </button>
      </div>

      <p className="text-xs text-muted-foreground">
        Profiles bundle a label, system-prompt instructions, and a tool
        whitelist. The default profile is used when a chat doesn't pick one.
      </p>

      <div className="flex flex-col md:flex-row gap-4">
        {/* Profile list */}
        <div className="md:w-56 shrink-0 border border-border rounded-md bg-panel">
          <ul className="max-h-72 md:max-h-[28rem] overflow-y-auto">
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
                    className={cn(
                      'w-full text-left px-3 py-2 text-sm transition flex items-center justify-between gap-2',
                      active
                        ? 'bg-primary/10 text-primary font-medium'
                        : 'text-foreground hover:bg-accent',
                    )}
                  >
                    <span className="truncate">
                      <span className="font-mono text-xs text-muted-foreground mr-1.5">
                        {id}
                      </span>
                      <span className="truncate">{p.label || '(unnamed)'}</span>
                    </span>
                    {isDefault && (
                      <span className="text-[10px] px-1.5 py-0.5 rounded border border-primary/30 bg-primary/10 text-primary shrink-0">
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
        <div className="flex-1 min-w-0">
          {selected && selectedId ? (
            <ProfileEditor
              id={selectedId}
              entry={selected}
              isDefault={draft.defaultProfileId === selectedId}
              labelTooLong={selected.label.length > LABEL_MAX}
              instructionsTooLong={selected.instructions.length > INSTRUCTIONS_MAX}
              onChange={updateSelected}
              onSetDefault={() => handleSetDefault(selectedId)}
              onDelete={() => handleDelete(selectedId)}
              canDelete={draft.defaultProfileId !== selectedId}
            />
          ) : (
            <div className="flex items-center justify-center h-32 text-xs text-muted-foreground italic border border-dashed border-border rounded-md">
              Select a profile to edit, or click Add.
            </div>
          )}
        </div>
      </div>

      {/* Default profile selector — full width below the list/editor. */}
      <div className="p-3 bg-panel border border-border rounded-md">
        <label
          htmlFor="default-profile-select"
          className="block text-xs text-muted-foreground mb-1"
        >
          Default profile
        </label>
        <select
          id="default-profile-select"
          value={draft.defaultProfileId}
          onChange={e => handleSetDefault(e.target.value)}
          className="w-full md:w-64 bg-background border border-input rounded-md px-2 py-1.5 text-sm"
        >
          {profileIds.map(id => (
            <option key={id} value={id}>
              {id} — {draft.profiles[id].label || '(unnamed)'}
            </option>
          ))}
        </select>
        <p className="text-[11px] text-muted-foreground mt-1">
          Used when a chat session doesn't pick a profile explicitly.
        </p>
      </div>

      {/* Error / success banner + Save / Reset. */}
      {error && (
        <div className="flex items-start gap-2 p-3 text-xs text-destructive bg-destructive/10 border border-destructive/30 rounded-md">
          <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
          <span className="font-mono whitespace-pre-wrap break-all">{error}</span>
        </div>
      )}

      <div className="flex items-center gap-2">
        <button
          onClick={handleSave}
          disabled={saving || !dirty || !!inlineError}
          className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-primary-foreground bg-primary hover:bg-primary/90 rounded-md transition disabled:opacity-50"
        >
          <Save className="w-3.5 h-3.5" />
          {saving ? 'Saving...' : 'Save'}
        </button>
        <button
          onClick={handleReset}
          disabled={saving || !dirty}
          className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-foreground bg-secondary hover:bg-accent rounded-md border border-input transition disabled:opacity-50"
        >
          <RotateCcw className="w-3.5 h-3.5" />
          Reset
        </button>
        {savedFlash && (
          <span className="flex items-center gap-1 text-xs text-green-500">
            <Check className="w-3.5 h-3.5" />
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
  onChange: (patch: Partial<ProfileEntry>) => void
  onSetDefault: () => void
  onDelete: () => void
  canDelete: boolean
}) {
  const toolsText = joinTools(entry.tools)
  const labelLen = entry.label.length
  const instrLen = entry.instructions.length

  return (
    <div className="p-4 bg-panel border border-border rounded-lg space-y-4">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-xs font-mono text-muted-foreground shrink-0">
            {id}
          </span>
          {isDefault && (
            <span className="text-[10px] px-1.5 py-0.5 rounded border border-primary/30 bg-primary/10 text-primary shrink-0">
              default
            </span>
          )}
        </div>
        <div className="flex items-center gap-2 shrink-0">
          {!isDefault && (
            <button
              onClick={onSetDefault}
              className="px-2.5 py-1 text-xs font-medium text-foreground bg-secondary hover:bg-accent rounded-md border border-input transition"
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
            className="flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium text-destructive bg-secondary hover:bg-destructive/10 rounded-md border border-destructive/30 transition disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Trash2 className="w-3.5 h-3.5" />
            Delete
          </button>
        </div>
      </div>

      {/* Label */}
      <div>
        <div className="flex items-baseline justify-between mb-1">
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
            'w-full bg-background border rounded-md px-3 py-1.5 text-sm',
            labelTooLong
              ? 'border-destructive focus:border-destructive'
              : 'border-input',
          )}
        />
        {labelTooLong && (
          <p className="text-[11px] text-destructive mt-1">
            Label exceeds the {LABEL_MAX}-character cap.
          </p>
        )}
      </div>

      {/* Instructions */}
      <div>
        <div className="flex items-baseline justify-between mb-1">
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
            'w-full bg-background border rounded-md px-3 py-2 text-sm font-mono resize-y',
            instructionsTooLong
              ? 'border-destructive focus:border-destructive'
              : 'border-input',
          )}
        />
        {instructionsTooLong && (
          <p className="text-[11px] text-destructive mt-1">
            Instructions exceed the {INSTRUCTIONS_MAX.toLocaleString()}-character cap.
          </p>
        )}
      </div>

      {/* Tools whitelist — text input for now; checkbox UI deferred to the
          S-PROF-TOOLS follow-up that exposes a GET /api/mcp/tools endpoint. */}
      <div>
        <label
          htmlFor={`profile-tools-${id}`}
          className="block text-xs text-muted-foreground mb-1"
        >
          Tools whitelist
        </label>
        <input
          id={`profile-tools-${id}`}
          type="text"
          value={toolsText}
          onChange={e => onChange({ tools: parseTools(e.target.value) })}
          placeholder="e.g. read_file, write_file, run_shell"
          className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm font-mono"
        />
        {/* TODO(S-PROF-TOOLS follow-up): replace this text input with
            per-server checkbox groups once GET /api/mcp/tools is exposed.
            Stale/unknown tools saved in a whitelist should still render
            (with a "stale" marker) so they aren't silently dropped. */}
        <p className="text-[11px] text-muted-foreground mt-1">
          Comma-separated tool names. Empty = allow all tools.
        </p>
      </div>
    </div>
  )
}
