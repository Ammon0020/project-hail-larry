# Dialog overlay uses raw color value instead of semantic token

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `web/src/components/ui/dialog.tsx`
- **Lines:** 34-37

## Description

DialogOverlay uses `bg-black/70` for the modal scrim. AGENTS.md > Tailwind CSS Standards explicitly requires semantic design tokens (`text-muted-foreground`, `bg-background`), not raw color values. The token registry in web/src/index.css defines `--color-background`, `--color-panel`, `--color-popover`, etc., but no overlay/backdrop token, so this is the one place in all five new UI files where a raw color leaks through. Functionally it works in both themes (black/70 darkens light and dark backgrounds alike), so this is a convention deviation rather than a visual bug — but it is the kind of inconsistency the project rule exists to prevent, and it sets a precedent for other components to reach for `bg-black/...` instead of a token.

## Recommendation

Add a semantic overlay token to web/src/index.css (e.g. `--overlay: rgba(0,0,0,0.7)` in both `:root` and `.dark`/`[data-theme=dark]`, plus `--color-overlay: var(--overlay)` in the `@theme` block), then use `bg-overlay` in DialogOverlay. If a theme-aware scrim is desired (e.g. a lighter scrim in dark mode), the token can differ per theme. Alternatively, if the team explicitly accepts black as the canonical scrim color, document the exception in AGENTS.md rather than leaving it as an untracked deviation.

## Verification

Grepped all five new UI files for raw Tailwind color utilities (`bg-(black|white|red|...|zinc|...)`, hex literals). Only one match: dialog.tsx:35 `bg-black/70`. Confirmed via web/src/index.css that no `--color-overlay`/`--color-backdrop` semantic token exists (only background/panel/popover/foreground/primary/secondary/muted/accent/destructive/border/input/ring). Cross-checked AGENTS.md lines 74-83 which state the semantic-token rule with no exception for overlays.
