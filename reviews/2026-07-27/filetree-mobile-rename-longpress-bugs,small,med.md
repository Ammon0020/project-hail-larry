- name: Mobile rename — long-press timer fires during inline rename and on unmounted rows
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/FileTree.tsx
- lines: 97-119, 249-269, 287, 383
- description: |
    Two mobile bugs in the rename/long-press interaction:

    1. **Long-press opens the context menu while renaming.** The rename
       `<input>` (line 249-268) is rendered *inside* the row `<div>` that
       carries `touchHandlers` (folder row line 287, file row line 383). The
       input stops `onClick` propagation (line 265) but does **not** stop
       `onTouchStart`/`onTouchEnd`. So a long-press on the input still starts
       the 500ms timer (line 110), which fires `openMenu` → `setMenuPath`,
       popping the context menu on top of the rename field mid-edit. On mobile
       this is easy to trigger by holding the finger while positioning the
       cursor.

    2. **Long-press timer is not cleared on unmount.** `useLongPressHandlers`
       (line 97-119) returns a `timer` ref and `clear`, but `TreeNode` never
       calls `clear` in a cleanup effect. If a row unmounts mid-long-press
       (e.g. the tree refreshes because a file was written/deleted, or the
       parent collapses the folder), the pending `setTimeout` still fires
       `openMenu`, which calls `setMenuPath` on a now-unmounted component —
       a stale-state/React-warning bug.

    Suggested fixes: add `onTouchStart={(e) => e.stopPropagation()}` (and
      touchEnd/Move) to the rename input so the parent timer doesn't start;
      add a `useEffect(() => () => clear(), [clear])` cleanup in
      `useLongPressHandlers` or in `TreeNode` so the timer is cleared on
      unmount.
- verification: |
    Read FileTree.tsx lines 97-119 (useLongPressHandlers — no unmount cleanup),
    249-269 (rename input — onClick stopPropagation only, no touch handlers),
    276-293 (folder row spreads {...touchHandlers} on the wrapping div that
    contains the input), 372-394 (same for file row). Confirmed the input is
    a child of the touch-handler-bearing div and has no touch-stop.
