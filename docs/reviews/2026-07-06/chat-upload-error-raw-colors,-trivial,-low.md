# Raw Tailwind color values in the upload-error banner violate the semantic-token standard

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `web/src/components/ChatPanel.tsx`
- **Lines:** 585-588

## Description

The new upload-error banner uses `border-red-500/40 bg-red-950/20 text-red-300` — hardcoded raw color utilities. AGENTS.md "Tailwind CSS Standards" explicitly requires semantic design tokens (`text-muted-foreground`, `bg-background`, `bg-destructive`, etc.) rather than raw color values, and the rest of the new attachment code in both `ChatPanel.tsx` and `ChatMessageItem.tsx` correctly uses `border-border`, `bg-muted`, `text-muted-foreground`, `hover:border-ring`. The existing `error` banner in this same component (rendered elsewhere) is the convention to mirror.

## Recommendation

Replace with semantic tokens, e.g. `border-destructive/40 bg-destructive/10 text-destructive` (matching how other error/destructive UI is styled in the codebase).

## Verification

Read line 586 — `border-red-500/40 bg-red-950/20 text-red-300`. Confirmed AGENTS.md rule "Use semantic design tokens (`text-muted-foreground`, `bg-background`), not raw color values". Compared with the new `ChatMessageItem.tsx` attachment markup (lines 144-154) which uses `border-border`, `bg-muted`, `text-muted-foreground` — consistent with the rule, making this banner the lone deviation in the diff.
