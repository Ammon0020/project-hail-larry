# ChatComposer profile selector has no accessible name

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `/media/adam/extex/projects/project-hail-larry/web/src/components/ChatComposer.tsx`
- **Lines:** 268-288

## Description

The profile selector is implemented as a native `<select>` overlaid transparently (`opacity-0`) on a styled `<div>` with a `Code` icon and a visible label span. The `<select>` itself has no `aria-label` and no associated `<label>`. The wrapping `<div>` only has `title="Profile context"`, which is not an accessible name for the control.

Every neighboring control in the same composer sets an explicit accessible name: `aria-label="Attach files"` (line 214), `aria-label="MCP tools"` with `aria-expanded` (lines 244-245), `aria-label="Stop"` (line 348), `aria-label="Send message"` (line 358). The profile selector is the only interactive control missing one, so a screen reader will announce it as an unlabeled combobox.

## Recommendation

Add `aria-label="Profile"` (or `aria-labelledby` pointing at the visible label span) to the `<select>` at line 276, and add `aria-expanded`/`aria-haspopup="listbox"` if you want parity with the MCP tools button. While there, also gate the wrapper's `cursor-pointer` and `hover:bg-white/[0.04]` behind `disabled={profiles.length === 0}` so the hitbox doesn't look interactive when the select is disabled.

## Verification

Inspected the profile selector block (lines 268-288): the `<select>` has only `value`, `onChange`, `disabled`, and `className` attributes — no `aria-label`, no `id`, no `<label htmlFor>`. Compared against the grep of `aria-` in `ChatComposer.tsx`, which shows every other interactive control has an `aria-label`.
