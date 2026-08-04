# Clean Up Redundant stopPropagation in GitPanel Action Buttons

## Problem

The git panel action buttons (Open File, Discard, Stage/Unstage) each have
`stopPropagation` on `onClick`, `onMouseDown`, and `onPointerDown`. The
container `<div>` wrapping all buttons already calls `stopPropagation` on
`onClick`, `onMouseDown`, and `onPointerDown`, making the per-button handlers
redundant. This adds noise without functional benefit.

## Scope

1. Keep `stopPropagation` on the container `<div>` for `onClick`,
   `onMouseDown`, and `onPointerDown`.
2. Remove redundant `stopPropagation` calls from individual button `onClick`,
   `onMouseDown`, and `onPointerDown` handlers (keep only the action logic).
3. Verify that clicking action buttons still does NOT trigger the row's
   `onOpenDiff` handler.

## Acceptance Criteria

- [ ] Container `<div>` retains `stopPropagation` on click/mouse/pointer.
- [ ] Individual buttons only contain their action logic (no redundant stops).
- [ ] Clicking any action button does not bubble to the row click handler.
- [ ] `npm run lint` passes.

## Verification

- Manual test: click Open File → opens file, NOT diff.
- Manual test: click Stage → stages, does NOT open diff.
- Manual test: click Discard → discards, does NOT open diff.
- `cd web && npm run lint` passes.
