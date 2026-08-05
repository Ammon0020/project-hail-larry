# S-GIT-CONTEXT-MENU-KB — Keyboard-navigable context menu

> **Status:** Pending. **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-viewer-v2-medium.md`.

## Goal

Replace the custom portal-based context menu with the Radix context menu
primitive so users get built-in keyboard arrow navigation, focus management,
and Escape-to-close.

## Scope

- Add `@radix-ui/react-context-menu` as a dependency.
- Replace `CommitContextMenu.tsx` with the Radix primitive.
- Keep the same items: Copy SHA, Open diff in tab, Checkout, Refresh.
- Preserve the existing trigger (right-click on a commit row) and styling
  (semantic tokens).

## Acceptance

- [ ] Context menu opens on right-click with the same four items.
- [ ] Arrow keys move focus between items; Enter activates; Escape closes.
- [ ] Focus returns to the triggering row on close.
- [ ] `make check` passes.
