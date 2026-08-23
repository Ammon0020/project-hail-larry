- name: CommandPalette results lack listbox/option semantics and screen-reader announcements
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/CommandPalette.tsx
- lines: 239-283
- description: |
    The result list container (line 239) and each result row (lines 246, 261)
    are plain `<div>`s with `onClick` and `data-idx`. There is no
    `role="listbox"` on the container, no `role="option"` / `aria-selected` on
    the rows, and no `aria-live` region to announce "No files found" or the
    result count. Screen-reader users cannot perceive the list as a list, hear
    which item is selected, or be notified when results change. The visible
    selection highlight (`bg-accent` on `i === selectedIndex`) is purely
    visual. Additionally, there is no Home/End key handling to jump to the
    first/last result (only ArrowUp/ArrowDown/Enter at lines 208-217). The
    dialog title is correctly `sr-only` (line 227) — good. Fix: add
    `role="listbox"` to the scroller, `role="option"` + `aria-selected={i ===
    selectedIndex}` to each row, and an `aria-live="polite"` status line for
    the count / empty state.
- verification: |
    Read CommandPalette.tsx lines 239-283: the container and rows are `<div>`
    elements with no ARIA roles. Lines 207-218 handle ArrowUp/Down/Enter only.
    The empty-state message at line 240-243 is a plain div with no
    `aria-live`.
