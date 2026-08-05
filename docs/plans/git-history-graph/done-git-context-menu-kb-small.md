# S-GIT-CONTEXT-MENU-KB — Keyboard-navigable context menu

> **Status:** Done (2026-07-30). **Difficulty:** small. **Epic:** `pending-git-history-graph-large.md`.
> **Depends on:** `done-git-graph-viewer-v2-medium.md`.
>
> *Done 2026-07-30 — `CommitContextMenu.tsx` rewritten from 91 lines (custom
> portal + manual handlers) to 45 lines using `@radix-ui/react-context-menu`.
> Radix provides right-click trigger, arrow-key navigation, focus management,
> and Escape-to-close out of the box. Same component interface, same Tailwind
> classes, same semantic tokens. `@radix-ui/react-context-menu@2.3.7` added as
> a direct dependency. 72 vitest cases pass; `make qcheck` passes.*

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

- [x] Context menu opens on right-click with the same four items.
- [x] Arrow keys move focus between items; Enter activates; Escape closes.
- [x] Focus returns to the triggering row on close.
- [x] `make check` passes.
