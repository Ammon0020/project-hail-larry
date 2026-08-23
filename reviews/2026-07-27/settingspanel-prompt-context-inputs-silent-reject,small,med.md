- name: Prompt-context number inputs silently reject out-of-range/empty values with no feedback
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/SettingsPanel.tsx
- lines: 279-283, 665-674
- description: `updatePromptContext` (line 281) does
  `if (!Number.isInteger(number) || number < 0 || number > 100) return` — a silent no-op.
  The inputs are `type="number" min={0} max={100}` (lines 666, 670), but `max` on a number
  input only constrains the spinner, not typed input. So a user who types "150" sees "1"
  and "15" appear, then on the third digit the controlled input snaps back to the previous
  value (15) with no message — looks like a stuck key. Likewise `Number("")` is `0`, so
  clearing the field to type a fresh value silently writes `0` instead of allowing an empty
  intermediate state. There is no inline validation hint (compare to `ProfileEditor` which
  shows "exceeds the N-character cap" under the field). The helper text "0 disables a list;
  maximum 100" (line 681) is below the Save button, not near the inputs. Fix: show an
  inline error under the offending input when the typed value is >100 or non-integer, and
  allow empty string as a transient state (only coerce to 0 on blur or save).
- verification: Read `SettingsPanel.tsx:279-283` (silent reject) and `665-674` (inputs).
  Confirmed no `aria-invalid`, no per-field error text, helper text is below the Save
  button at line 681.
