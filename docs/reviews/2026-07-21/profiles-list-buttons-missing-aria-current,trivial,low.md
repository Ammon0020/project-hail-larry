# Profile list buttons missing aria-pressed / aria-current

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `/media/adam/extex/projects/project-hail-larry/web/src/components/ProfilesSettings.tsx`
- **Lines:** 307-328

## Description

The profile list renders each entry as a `<button>` whose active/selected state is conveyed only through visual styling (`active ? 'bg-primary/10 text-primary font-medium' : ...` at lines 312-314). There is no `aria-pressed`, `aria-current`, or role indication that communicates the selected entry to assistive technology. A screen reader user navigating the list hears a set of buttons with profile names but cannot tell which one is currently selected for editing.

For comparison, the favorite toggle in `ChatComposer` (lines 303-304) correctly uses `aria-pressed={isFavorite}` for a similar visual-only state.

## Recommendation

Add `aria-current={active ? 'true' : undefined}` (or `aria-pressed={active}`) to the profile list `<button>` at line 308. `aria-current="true"` is the more semantically correct choice for a "currently selected item" pattern. Consider also wrapping the `<ul>` with `role="listbox"` and the items with `role="option"` plus `aria-selected` if you want full listbox semantics, but `aria-current` is the minimal fix.

## Verification

Read the list item button (lines 307-328): the `<button>` has only `onClick` and `className` attributes — no `aria-pressed`, `aria-current`, or `role`. The `active` boolean is computed at line 304 and used solely for the `cn(...)` class merge. Cross-checked `ChatComposer.tsx` line 304 which uses `aria-pressed={isFavorite}` for an analogous toggle.
