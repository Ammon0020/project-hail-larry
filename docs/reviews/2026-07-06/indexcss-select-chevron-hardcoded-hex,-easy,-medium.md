# Select chevron SVGs use hardcoded hex strokes that do not adapt to theme

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `web/src/index.css`
- **Lines:** 135-144

## Description

`.select-chevron` embeds stroke `%239ca3af` (#9ca3af) and `.select-chevron-primary` embeds `%2360a5fa` (#60a5fa) inside data-URI SVGs. The preceding comment (lines 130-134) claims 'The stroke color uses the muted-foreground token so the chevron adapts to theme changes' — but a CSS custom property cannot be substituted inside a url() data-URI SVG, so the value is a raw hex. In light mode --muted-foreground is #64748b (line 40), so the chevron renders as #9ca3af (the dark-mode value) regardless of theme. This violates AGENTS.md 'Tailwind CSS Standards' ('Use semantic design tokens ... not raw color values') and produces a visible mismatch in light mode.

## Recommendation

Replace the data-URI background-image approach with an inline SVG element (so `stroke='currentColor'` works) or a CSS mask with `background-color: var(--muted-foreground)` / `var(--primary)`. Either approach lets the chevron inherit the semantic token and adapt to theme.

## Verification

Read index.css lines 135-144 (hardcoded %239ca3af / %2360a5fa in url()). Read index.css lines 40 and 70 confirming --muted-foreground differs between light (#64748b) and dark (#9ca3af) — so the chevron only matches dark. Read AGENTS.md Tailwind CSS Standards requiring semantic tokens over raw color values.
