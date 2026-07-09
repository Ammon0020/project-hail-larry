# Ctrl+B sidebar toggle discards the user's custom width

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `web/src/App.tsx`
- **Lines:** 422-426

## Description

Ctrl+B toggles the left sidebar via `setLeftPanelWidth((prev) => (prev > 0 ? 0 : 260))`. When the sidebar is hidden (prev === 0) it always restores to 260, not the user's previously resized width. A user who dragged the sidebar to 400px, hit Ctrl+B to hide, then Ctrl+B again, gets 260px — silently losing their layout. The persisted width (localStorage effect at line 226) then overwrites the saved 400 with 260, so the loss survives reload.

## Recommendation

Stash the pre-hide width in a ref or state before zeroing, and restore from that stash instead of the hardcoded 260. e.g. `setLeftPanelWidth((prev) => { if (prev > 0) { hiddenWidthRef.current = prev; return 0 } return hiddenWidthRef.current ?? 260 })`.

## Verification

Read App.tsx lines 422-426 — the restore branch is the literal `260`, not a stored prior value. Read App.tsx lines 226-228 confirming the width is persisted to localStorage on every change, so the clobbered 260 is saved.
