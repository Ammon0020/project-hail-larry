import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ChevronDown, Search, Star, Zap } from 'lucide-react'
import { cn } from '../../lib/utils'
import {
  groupModelsByBase,
  thinkingLabel,
  THINKING_LEVELS,
  type BaseModelGroup,
  type ModelVariant,
  type ThinkingLevel,
} from '../../lib/modelGrouping'
import {
  getModelPrefs,
  togglePinnedModel,
  type ModelOption,
  type ModelPrefs,
} from '../../lib/modelPrefs'

interface ModelSelectorProps {
  /** Available models for the current agent. */
  models: ModelOption[]
  /** Active harness id — scopes pinned/recent in localStorage. */
  agentId: string
  /** Currently selected model variant ID. */
  selectedModelId: string
  /** Called when the user selects a model variant. */
  onModelChange: (id: string) => void
  /** Whether the selector is disabled (no models, etc.). */
  disabled?: boolean
}

/**
 * Model selector with an anchored-above-composer popover.
 *
 * Models are grouped by base id (`groupModelsByBase`). Each base model renders
 * as a compact block with inline thinking-level pills and, where available, a
 * "Fast" toggle pill next to each thinking level. Pinned base models are
 * hoisted to a top section; the rest follow alphabetically.
 *
 * Outside-click and Escape close the popover (same pattern as `McpPopout`).
 * The trigger button is marked with `data-model-selector-toggle` so the
 * popover's outside-click handler can ignore clicks on it.
 */
export function ModelSelector({
  models,
  agentId,
  selectedModelId,
  onModelChange,
  disabled,
}: ModelSelectorProps) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const [prefs, setPrefs] = useState<ModelPrefs>(() => getModelPrefs(agentId))
  // Track the agent id that `prefs` was loaded for so we can refresh prefs
  // when the prop changes without an effect (see render-time sync below).
  const [prefsAgentId, setPrefsAgentId] = useState(agentId)

  const popoverRef = useRef<HTMLDivElement>(null)
  const searchInputRef = useRef<HTMLInputElement>(null)

  // Refresh prefs when the agent prop changes. This is React's documented
  // "adjust state during render" pattern for syncing state to a prop without
  // an effect — see https://react.dev/learn/you-might-not-need-an-effect
  // #adjusting-some-state-when-a-prop-changes. Setting state during render
  // for the same component is safe and re-renders immediately without an
  // extra commit.
  if (prefsAgentId !== agentId) {
    setPrefsAgentId(agentId)
    setPrefs(getModelPrefs(agentId))
  }

  // Base-model groups derived from the flat model list. Recomputed only when
  // the model list changes.
  const baseGroups = useMemo(() => groupModelsByBase(models), [models])

  // Currently selected variant, looked up so we can highlight the matching
  // thinking pill within its base block.
  const selectedGroup = useMemo(
    () => baseGroups.find((g) => g.variants.some((v) => v.modelId === selectedModelId)),
    [baseGroups, selectedModelId],
  )

  // Filter base groups by the search query. Matches against the group display
  // name or the base id (case-insensitive). An empty query shows everything.
  const filteredGroups = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return baseGroups
    return baseGroups.filter(
      (g) =>
        g.displayName.toLowerCase().includes(q) || g.baseId.toLowerCase().includes(q),
    )
  }, [baseGroups, search])

  // Partition filtered groups into pinned (hoisted) and the rest. Pinned
  // preserves the user's pin order; the rest are alphabetical (already sorted
  // by `groupModelsByBase`).
  const { pinnedGroups, otherGroups } = useMemo(() => {
    const pinnedSet = new Set(prefs.pinned)
    const pinned: BaseModelGroup[] = []
    const other: BaseModelGroup[] = []
    // Walk pinned base ids in order so the Pinned section reflects pin order
    // rather than alphabetical.
    for (const baseId of prefs.pinned) {
      const g = filteredGroups.find((grp) => grp.baseId === baseId)
      if (g) pinned.push(g)
    }
    for (const g of filteredGroups) {
      if (!pinnedSet.has(g.baseId)) other.push(g)
    }
    return { pinnedGroups: pinned, otherGroups: other }
  }, [filteredGroups, prefs.pinned])

  // Auto-focus the search input when the popover opens.
  useEffect(() => {
    if (!open) return
    // Defer to next tick so the input is mounted before we focus it.
    const t = window.setTimeout(() => searchInputRef.current?.focus(), 0)
    return () => window.clearTimeout(t)
  }, [open])

  // Centralized close: clears the search query so reopening starts fresh,
  // and closes the popover. Used by the trigger, outside-click, Escape, and
  // pill-select paths so we never need to setState-on-close inside an effect.
  const closePopover = useCallback(() => {
    setOpen(false)
    setSearch('')
  }, [])

  // Outside-click + Escape close. Mirrors McpPopout's pattern: the trigger
  // button is marked with `data-model-selector-toggle` so clicks on it don't
  // cause a close-then-reopen flicker (its own onClick handles toggling).
  useEffect(() => {
    if (!open) return
    function handleClickOutside(e: MouseEvent) {
      const target = e.target as Element | null
      if (!target || !popoverRef.current) return
      if (popoverRef.current.contains(target)) return
      if (
        typeof target.closest === 'function' &&
        target.closest('[data-model-selector-toggle]')
      ) {
        return
      }
      closePopover()
    }
    function handleEscape(e: KeyboardEvent) {
      if (e.key === 'Escape') closePopover()
    }
    document.addEventListener('mousedown', handleClickOutside)
    document.addEventListener('keydown', handleEscape)
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
      document.removeEventListener('keydown', handleEscape)
    }
  }, [open, closePopover])

  const handleTogglePin = useCallback(
    (baseId: string) => {
      setPrefs(togglePinnedModel(agentId, baseId))
    },
    [agentId],
  )

  const handleSelectVariant = useCallback(
    (modelId: string) => {
      onModelChange(modelId)
      closePopover()
    },
    [onModelChange, closePopover],
  )

  // Trigger label: the selected model's display name, falling back to a
  // placeholder when there are no models.
  const triggerLabel = useMemo(() => {
    const selected = models.find((m) => m.id === selectedModelId)
    return selected?.name ?? (models.length === 0 ? 'No models' : 'Select model')
  }, [models, selectedModelId])

  const isDisabled = disabled || models.length === 0

  return (
    <div className="relative">
      {/* Trigger button — compact muted label with a chevron, same visual
          style as the rest of the composer hitbox labels. */}
      <button
        type="button"
        data-model-selector-toggle
        disabled={isDisabled}
        onClick={() => (open ? closePopover() : setOpen(true))}
        className={cn(
          'flex items-center gap-1 max-w-[10rem] px-1.5 py-0.5 rounded text-xs text-muted-foreground',
          'hover:bg-accent hover:text-foreground transition-colors',
          'disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-transparent',
        )}
        aria-haspopup="listbox"
        aria-expanded={open}
      >
        <span className="truncate">{triggerLabel}</span>
        <ChevronDown
          className={cn(
            'w-3 h-3 shrink-0 transition-transform',
            open && 'rotate-180',
          )}
        />
      </button>

      {open && !isDisabled && (
        <div
          ref={popoverRef}
          role="listbox"
          className="absolute bottom-full left-0 mb-2 w-[320px] z-50 bg-popover border border-border rounded-[10px] shadow-lg flex flex-col"
        >
          {/* Search input — auto-focused on open. */}
          <div className="p-2 border-b border-border">
            <div className="relative">
              <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground pointer-events-none" />
              <input
                ref={searchInputRef}
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search models"
                className={cn(
                  'w-full pl-7 pr-2 py-1.5 text-xs rounded-md',
                  'bg-secondary/60 border border-border text-foreground',
                  'placeholder:text-muted-foreground',
                  'focus:outline-none focus:ring-1 focus:ring-primary/40',
                )}
              />
            </div>
          </div>

          {/* Scrollable list. */}
          <div className="max-h-[400px] overflow-y-auto p-2 flex flex-col gap-3">
            {pinnedGroups.length === 0 && otherGroups.length === 0 && (
              <div className="text-xs text-muted-foreground py-4 text-center">
                No models match "{search.trim()}"
              </div>
            )}

            {pinnedGroups.length > 0 && (
              <Section title="Pinned">
                {pinnedGroups.map((group) => (
                  <BaseModelBlock
                    key={group.baseId}
                    group={group}
                    isPinned
                    isSelectedGroup={selectedGroup?.baseId === group.baseId}
                    selectedModelId={selectedModelId}
                    onTogglePin={handleTogglePin}
                    onSelectVariant={handleSelectVariant}
                  />
                ))}
              </Section>
            )}

            {otherGroups.length > 0 && (
              <Section title={pinnedGroups.length > 0 ? 'All Models' : 'Models'}>
                {otherGroups.map((group) => (
                  <BaseModelBlock
                    key={group.baseId}
                    group={group}
                    isPinned={false}
                    isSelectedGroup={selectedGroup?.baseId === group.baseId}
                    selectedModelId={selectedModelId}
                    onTogglePin={handleTogglePin}
                    onSelectVariant={handleSelectVariant}
                  />
                ))}
              </Section>
            )}
          </div>
        </div>
      )}
    </div>
  )
}

/** A labeled section of base-model blocks. */
function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="text-[10px] uppercase tracking-wide text-muted-foreground px-1">
        {title}
      </div>
      {children}
    </div>
  )
}

interface BaseModelBlockProps {
  group: BaseModelGroup
  isPinned: boolean
  isSelectedGroup: boolean
  selectedModelId: string
  onTogglePin: (baseId: string) => void
  onSelectVariant: (modelId: string) => void
}

/**
 * A single base-model block: header (display name + pin + preferred badge)
 * followed by inline thinking-level pills and per-level Fast toggles.
 *
 * For a base with a single variant (no thinking levels, no fast), we render
 * just one pill labeled with the display name.
 */
function BaseModelBlock({
  group,
  isPinned,
  isSelectedGroup,
  selectedModelId,
  onTogglePin,
  onSelectVariant,
}: BaseModelBlockProps) {
  // Index variants by thinking level so we can render one pill per level and
  // look up the fast/non-fast variant for each.
  const byLevel = useMemo(() => indexVariantsByLevel(group.variants), [group.variants])
  const levels = useMemo(() => orderedLevelsForGroup(group.variants), [group.variants])
  const isSingleVariant = group.variants.length === 1

  return (
    <div
      className={cn(
        'rounded-md px-1.5 py-1.5 flex flex-col gap-1.5',
        isSelectedGroup && 'bg-accent/40',
      )}
    >
      {/* Header row: display name + preferred badge + pin icon. */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5 min-w-0">
          <span className="text-xs font-medium truncate">{group.displayName}</span>
          {group.preferred && (
            <span className="text-[9px] uppercase tracking-wide text-primary shrink-0">
              Preferred
            </span>
          )}
        </div>
        <button
          type="button"
          onClick={() => onTogglePin(group.baseId)}
          className={cn(
            'p-0.5 rounded shrink-0 transition-colors',
            isPinned
              ? 'text-primary hover:text-primary/80'
              : 'text-muted-foreground hover:text-foreground hover:bg-accent',
          )}
          aria-label={isPinned ? `Unpin ${group.displayName}` : `Pin ${group.displayName}`}
          aria-pressed={isPinned}
          title={isPinned ? 'Unpin' : 'Pin'}
        >
          <Star
            className={cn('w-3 h-3', isPinned && 'fill-current')}
          />
        </button>
      </div>

      {/* Variant pills. */}
      <div className="flex flex-wrap gap-1">
        {isSingleVariant ? (
          <ThinkingPill
            label={group.variants[0].name}
            selected={group.variants[0].modelId === selectedModelId}
            onClick={() => onSelectVariant(group.variants[0].modelId)}
          />
        ) : (
          levels.map((level) => {
            const entry = byLevel.get(level)
            if (!entry) return null
            const baseVariant = entry.nonFast ?? entry.fast
            if (!baseVariant) return null
            const fastVariant = entry.fast
            return (
              <div key={level} className="flex items-center gap-1">
                <ThinkingPill
                  label={thinkingLabel(level)}
                  selected={baseVariant.modelId === selectedModelId}
                  onClick={() => onSelectVariant(baseVariant.modelId)}
                />
                {fastVariant && (
                  <FastToggle
                    selected={fastVariant.modelId === selectedModelId}
                    onClick={() => onSelectVariant(fastVariant.modelId)}
                  />
                )}
              </div>
            )
          })
        )}
      </div>
    </div>
  )
}

/** A single thinking-level pill. */
function ThinkingPill({
  label,
  selected,
  onClick,
}: {
  label: string
  selected: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'text-[11px] px-2 py-0.5 rounded-full transition-colors',
        selected
          ? 'bg-primary text-primary-foreground'
          : 'bg-secondary text-secondary-foreground hover:bg-accent',
      )}
    >
      {label}
    </button>
  )
}

/** A small "Fast" toggle pill shown next to a thinking level. */
function FastToggle({ selected, onClick }: { selected: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label="Fast variant"
      title="Fast"
      className={cn(
        'inline-flex items-center gap-0.5 text-[10px] px-1.5 py-0.5 rounded-full transition-colors',
        selected
          ? 'bg-primary text-primary-foreground'
          : 'bg-secondary/70 text-muted-foreground hover:bg-accent hover:text-foreground',
      )}
    >
      <Zap className="w-2.5 h-2.5" />
      <span>Fast</span>
    </button>
  )
}

/**
 * Index variants by thinking level, separating the fast and non-fast variant
 * for each level. A level may have only a fast variant, only a non-fast
 * variant, or both.
 */
function indexVariantsByLevel(
  variants: ModelVariant[],
): Map<ThinkingLevel, { nonFast?: ModelVariant; fast?: ModelVariant }> {
  const map = new Map<ThinkingLevel, { nonFast?: ModelVariant; fast?: ModelVariant }>()
  for (const v of variants) {
    const level = v.thinking ?? 'none'
    const entry = map.get(level) ?? {}
    if (v.fast) entry.fast = v
    else entry.nonFast = v
    map.set(level, entry)
  }
  return map
}

/**
 * Ordered list of thinking levels present in a group, following the canonical
 * THINKING_LEVELS order. Variants without an explicit thinking level are
 * treated as 'none'.
 */
function orderedLevelsForGroup(variants: ModelVariant[]): ThinkingLevel[] {
  const present = new Set<ThinkingLevel>()
  for (const v of variants) present.add(v.thinking ?? 'none')
  return THINKING_LEVELS.filter((lvl) => present.has(lvl))
}
