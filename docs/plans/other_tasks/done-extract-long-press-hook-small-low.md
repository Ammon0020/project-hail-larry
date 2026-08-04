# Extract Shared useLongPressHandlers Hook

## Problem

`useLongPressHandlers` is duplicated between `FileTree.tsx` (canonical) and
`GitPanel.tsx` (copy). The GitPanel copy's own comment (line 48) acknowledges
the duplication. Any bug fix or behavior change must be applied in both places.

## Scope

1. Extract `useLongPressHandlers` to `web/src/hooks/useLongPressHandlers.ts`.
2. Update `FileTree.tsx` and `GitPanel.tsx` to import from the shared hook.
3. Remove the duplicate implementations and the "duplicated here" comment.

## Acceptance Criteria

- [ ] Single `useLongPressHandlers` definition in `web/src/hooks/`.
- [ ] Both `FileTree.tsx` and `GitPanel.tsx` import from the shared hook.
- [ ] No behavioral change — long-press context menus still work in both
      the file explorer and the git panel.
- [ ] `npm run lint` and `npm run build` pass.

## Verification

- Manual test: long-press a file in the explorer → context menu opens.
- Manual test: long-press a file in the git panel → context menu opens.
- `cd web && npm run lint && npm run build` passes.
