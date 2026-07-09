# Reconnecting and save-error banners not announced to screen readers

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `web/src/App.tsx`
- **Lines:** 618-638

## Description

The 'Reconnecting…' banner (lines 618-623) and the 'Save failed: …' banner (lines 626-638) render as plain <div>s with no role='alert' or aria-live region. The save-error dismiss button has an aria-label, but the banner content itself is not announced when it appears. Screen reader users get no notification of a dropped connection or a failed save — both are urgent, time-sensitive states.

## Recommendation

Add `role='alert'` (or `aria-live='polite'` for the reconnecting banner, since it is informational and transient) to each banner wrapper so assistive tech announces the state change.

## Verification

Read App.tsx lines 618-638 — neither banner div carries role, aria-live, or aria-atomic attributes.
