# LockScreen Enter key can fire concurrent pair requests while loading

- **Difficulty:** easy
- **Urgency:** low
- **File:** `web/src/components/LockScreen.tsx`
- **Lines:** 20-48, 82-88

## Description

`handleKeyDown` (lines 46-48) calls `attemptPair()` on Enter without checking the `loading` state, and `attemptPair` itself (lines 20-44) has no re-entrancy guard. The "Pair Device" button is disabled during loading (line 84), but the text input's `onKeyDown` bypasses that disabled state. A user pressing Enter twice (or Enter while the button is mid-request) launches two concurrent `api.verifyPasscode` calls; whichever resolves second wins the `localStorage.setItem('lai:deviceCredential', …)` write and the `onPaired()` callback may fire twice. The diff rewrote the surrounding markup onto semantic tokens but left this control-flow gap.

## Recommendation

Guard at the top of `attemptPair`: `if (loading) return` — or guard in `handleKeyDown` (`if (e.key === 'Enter' && !loading) attemptPair()`). The early-return in `attemptPair` is the more defensive option since it also covers any future direct callers.

## Verification

Read `LockScreen.tsx` lines 20-48 and 82-88; `loading` is in scope of `handleKeyDown` (it closes over the current render's state) but is never consulted there, and `attemptPair` only sets `loading` true after the format check, with no check for an already-in-flight request.
