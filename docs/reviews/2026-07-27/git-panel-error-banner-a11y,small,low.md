- name: Error banner is not announced to screen readers and never auto-clears
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/git/GitPanel.tsx
- lines: 254
- description: |
    The error banner at line 254 is a plain `<div>` with no `role`/`aria-live`:

    ```
    <div className="mx-3 mt-3 rounded-md border border-destructive/30 bg-destructive/10 px-2 py-1.5 text-xs text-destructive">{error}</div>
    ```

    Screen reader users get no announcement when a stage/commit/push fails — the error is
    only visible. Compare with `GitDiffViewer.tsx` line 184, which correctly marks its
    truncated banner `role="status" aria-live="polite"`.

    Two minor related issues in the same area:
    - The error is only cleared by `setError(null)` at the start of `runMutation` (line 160)
      and at the start of `refreshStatus` (line 145). A successful manual refresh clears it,
      but there is no dismiss control on the banner itself, so a stale error from a failed
      commit sits visible until the user takes another action.
    - There is no `aria-busy` on the panel or the action buttons during mutations, so AT
      users don't know the buttons are temporarily non-functional (the `disabled` attribute
      does prevent activation but doesn't announce "busy").

    Fix: add `role="alert"` to the error div (errors are assertive), and consider a small
    dismiss × button. Add `aria-busy={busy}` to the panel root.
- verification: |
    Read line 254: the error div has only `className`, no `role` or `aria-live`. Contrast
    with `GitDiffViewer.tsx` line 184 which uses `role="status" aria-live="polite"`. The
    only places `setError(null)` is called are `refreshStatus` (145) and `runMutation` (160)
    — there is no dismiss affordance on the banner.
