- name: Mobile left sidebar is a full-screen overlay with no backdrop, Escape, or focus trap
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/LeftSidebar.tsx
- lines: 70-78
- description: |
    On mobile, the sidebar is rendered as `absolute inset-0 z-30` (line 75)
    covering the entire viewport when `mobileView === 'explorer'`. There is:
      * No backdrop behind the sidebar (it IS the view, so a backdrop is
        arguably not needed) — but also no way to dismiss it other than
        tapping a different MobileNav item. There is no close button, no
        Escape handler, and tapping a file selects it but does *not* switch
        back to the editor view automatically on desktop; on mobile
        `handleFileSelect` does call `setMobileView('editor')` (App.tsx line
        733) so tapping a file dismisses the sidebar — good — but tapping a
        folder to expand it does not, and there is no visible "back to
        editor" affordance.
      * No focus trap: when the sidebar opens, focus stays wherever it was
        (often the bottom nav). A keyboard / switch user cannot get into the
        file tree without tabbing through everything above it.
      * The `<aside>` has no `aria-label` to distinguish it from other
        landmark regions.
    The mini activity bar at lines 90-105 also uses `<button>`s without
    `aria-pressed` (the active panel is conveyed only by color), which is a
    minor a11y gap for the same control that ActivityBar.tsx gets right.
    Recommend: add `aria-label="Explorer panel"` to the aside, `aria-pressed`
    to the mini tabs, and a visible "Back to editor" header button on mobile
    (the workspace header already occupies the top, but a chevron-back would
    help). A focus trap is the larger fix.
- verification: |
    Read LeftSidebar.tsx lines 70-78: the aside has no ARIA label and no
    key/focus handling. Lines 90-104: mini-tab buttons have no `aria-pressed`.
    Confirmed App.tsx line 733 switches to editor on file select, so a file
    tap does dismiss — but folder expand/collapse and "no file selected yet"
    leave the user trapped in the overlay.
