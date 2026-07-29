- name: WorkspaceHeader status button missing aria-expanded and aria-haspopup
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/WorkspaceHeader.tsx
- lines: 37-61
- description: |
    The connection-status pill is a `<button>` that toggles an informational
    dropdown (lines 63-78), but unlike the workspace-selector button on line
    95-96 which correctly sets `aria-expanded` and `aria-haspopup="listbox"`,
    the status button has neither attribute. Screen-reader users get no
    indication that the button opens a popover, nor whether it is currently
    open. The dropdown panel itself (lines 66-76) also has no `role` and is
    not announced as a dialog/region. Minor but inconsistent with the sibling
    control in the same component. While here, consider giving the dropdown a
    small `role="dialog"` or `aria-labelledby` and moving focus into it on
    open, since it contains informational text the user is expected to read.
- verification: |
    Read WorkspaceHeader.tsx lines 37-61: the status `<button>` has `title`
    and `onClick` but no `aria-expanded` / `aria-haspopup`. Compare lines
    88-103 which set both attributes on the workspace button.
