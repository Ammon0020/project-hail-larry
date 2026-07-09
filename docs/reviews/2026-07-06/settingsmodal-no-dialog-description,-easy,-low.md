# SettingsModal lacks accessible description

- **Difficulty:** easy
- **Urgency:** low
- **File:** `web/src/components/SettingsModal.tsx`
- **Lines:** 71-80

## Description

The migration from a hand-rolled `<div role="dialog">` to Radix `Dialog` dropped the implicit labeling but only provides a `DialogTitle` ("Settings"). There is no `DialogDescription`, so screen readers announce only the title with no summary of the dialog's purpose. The code passes `aria-describedby={undefined}` (line 74) to silence Radix's built-in warning — that is the documented way to suppress the warning *when a description is intentionally omitted*, but here the omission is a regression from the prior markup, which had `aria-label="Settings"` and a visible header. The `ui/dialog.tsx` wrapper exports `DialogDescription` and would render it as a `text-muted-foreground` sr-style line for free.

## Recommendation

Add a visually-hidden `<DialogDescription className="sr-only">Manage configured agents and general preferences.</DialogDescription>` inside `DialogHeader` (or a visible one if desired), and drop the `aria-describedby={undefined}` prop so Radix wires it automatically.

## Verification

Read `SettingsModal.tsx` lines 1-80 (only `Dialog`, `DialogContent`, `DialogHeader`, `DialogTitle` are imported/used; `DialogDescription` is exported by `ui/dialog.tsx` line 124 but not imported). Confirmed `aria-describedby={undefined}` is present at line 74.
