# Changed-on-disk banner uses raw amber-* palette colors instead of semantic tokens

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `web/src/components/EditorPane.tsx`
- **Lines:** 305-310, 372-385

## Description

The newly added "changed on disk" UI uses raw Tailwind palette classes — `text-amber-600 dark:text-amber-400` on the tab icon (line 307) and `bg-amber-500/10 border-amber-500/40 text-amber-700 dark:text-amber-300` / `text-amber-800 dark:text-amber-200 bg-amber-500/15 hover:bg-amber-500/25` on the banner (lines 373, 381). AGENTS.md ("Tailwind CSS Standards") explicitly requires semantic design tokens (`text-muted-foreground`, `bg-background`) rather than raw color values, and the rest of this very diff is a migration *away* from `text-gray-*`/`text-blue-*` onto tokens. `index.css` defines `--destructive` but no `--warning`/`--caution` token, so the author reached for raw amber instead. The inline comment even acknowledges "Amber is an intentional warning signal" — that intent should be encoded as a theme token so it adapts to future palettes and the light/dark surfaces consistently.

## Recommendation

Add a `--warning` / `--warning-foreground` pair to `:root` and `.dark` in `web/src/index.css`, map them in `@theme inline` (`--color-warning: var(--warning)`), then use `bg-warning/10 border-warning/40 text-warning` (and a `dark:`-free variant) in the banner and tab icon. This keeps the new feature consistent with the token migration happening across the rest of the diff.

## Verification

Confirmed via `web/src/index.css` that no `warning`/`amber` semantic token exists (only `--destructive`, `--primary`, `--ring`, etc. are mapped in `@theme inline`). Confirmed via the staged diff that these `amber-*` classes are newly added (the surrounding lines migrated `text-gray-*` → `text-muted-foreground`).
