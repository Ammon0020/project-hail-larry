import { WrapText, ListOrdered, ChevronsLeftRightEllipsis, Brackets, IndentIncrease, Braces } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { EditorSettings } from '@/hooks/useEditorSettings'

/**
 * EditorSettings — controlled UI for the editor preferences managed by
 * {@link useEditorSettings}. The parent (SettingsPanel) owns the settings
 * object and an `onChange` patch callback, so this component stays stateless
 * and re-renders purely from props.
 *
 * Styling follows the SettingsPanel card pattern (bg-panel border rounded-lg)
 * and uses semantic tokens so it adapts to light/dark themes.
 */
export function EditorSettings({
  settings,
  onChange,
}: {
  settings: EditorSettings
  onChange: (patch: Partial<EditorSettings>) => void
}) {
  return (
    <div className="p-4 bg-panel border border-border rounded-lg space-y-4">
      <div>
        <h4 className="font-semibold text-sm text-foreground">Editor</h4>
        <p className="mt-1 text-xs text-muted-foreground">
          Customize the code editor's appearance and editing behavior.
        </p>
      </div>

      {/* Font size — slider + numeric readout (8–32px). */}
      <label className="space-y-1.5">
        <div className="flex items-center justify-between">
          <span className="text-sm text-foreground">Font size</span>
          <span className="text-xs tabular-nums text-muted-foreground">{settings.fontSize}px</span>
        </div>
        <input
          type="range"
          min={8}
          max={32}
          value={settings.fontSize}
          onChange={(e) => onChange({ fontSize: Number(e.target.value) })}
          className="w-full accent-primary cursor-pointer"
        />
      </label>

      {/* Tab size — select 1/2/4/8 spaces. */}
      <label className="space-y-1.5">
        <span className="text-sm text-foreground">Tab size</span>
        <select
          value={settings.tabSize}
          onChange={(e) => onChange({ tabSize: Number(e.target.value) })}
          className="w-full bg-background border border-input rounded-md px-3 py-1.5 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
        >
          {[1, 2, 4, 8].map((n) => (
            <option key={n} value={n}>{n} spaces</option>
          ))}
        </select>
      </label>

      {/* Toggles — word wrap, line numbers, fold gutter, bracket matching,
          auto indent, close brackets. Reusable row layout keeps the list
          scannable on mobile and desktop. */}
      <div className="space-y-1">
        <ToggleRow
          icon={<WrapText className="w-4 h-4" />}
          label="Word wrap"
          description="Wrap long lines instead of horizontal scrolling."
          checked={settings.wrap}
          onChange={(v) => onChange({ wrap: v })}
        />
        <ToggleRow
          icon={<ListOrdered className="w-4 h-4" />}
          label="Line numbers"
          description="Show line numbers in the gutter."
          checked={settings.lineNumbers}
          onChange={(v) => onChange({ lineNumbers: v })}
        />
        <ToggleRow
          icon={<ChevronsLeftRightEllipsis className="w-4 h-4" />}
          label="Fold gutter"
          description="Show the fold gutter for collapsing code blocks. Disabled on mobile regardless."
          checked={settings.foldGutter}
          onChange={(v) => onChange({ foldGutter: v })}
        />
        <ToggleRow
          icon={<Brackets className="w-4 h-4" />}
          label="Bracket matching"
          description="Highlight the matching bracket pair around the cursor."
          checked={settings.bracketMatching}
          onChange={(v) => onChange({ bracketMatching: v })}
        />
        <ToggleRow
          icon={<IndentIncrease className="w-4 h-4" />}
          label="Auto indent"
          description="Re-indent lines as you type (smart indent on Enter)."
          checked={settings.autoIndent}
          onChange={(v) => onChange({ autoIndent: v })}
        />
        <ToggleRow
          icon={<Braces className="w-4 h-4" />}
          label="Close brackets"
          description="Automatically close brackets and quotes."
          checked={settings.closeBrackets}
          onChange={(v) => onChange({ closeBrackets: v })}
        />
      </div>
    </div>
  )
}

/**
 * ToggleRow — a single labeled switch row. Uses a checkbox styled as a switch
 * via accent-color, matching the radio/checkbox affordances elsewhere in
 * SettingsPanel. Kept local because it is specific to this settings list.
 */
function ToggleRow({
  icon,
  label,
  description,
  checked,
  onChange,
}: {
  icon: React.ReactNode
  label: string
  description: string
  checked: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <label
      className={cn(
        'flex items-start gap-3 py-2 px-2 -mx-2 rounded-md cursor-pointer transition',
        'hover:bg-accent/50',
      )}
    >
      <span className="mt-0.5 text-muted-foreground shrink-0">{icon}</span>
      <div className="flex-1 min-w-0 space-y-0.5">
        <span className="block text-sm text-foreground">{label}</span>
        <span className="block text-xs text-muted-foreground">{description}</span>
      </div>
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="mt-1 h-4 w-4 rounded border-input accent-primary cursor-pointer shrink-0"
      />
    </label>
  )
}
