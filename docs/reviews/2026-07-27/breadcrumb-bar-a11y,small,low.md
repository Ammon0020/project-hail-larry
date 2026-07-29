- name: BreadcrumbBar has no ARIA label and segments are not navigable
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/BreadcrumbBar.tsx
- lines: 23-49
- description: |
    The breadcrumb is a plain `<div>` (line 24) with no `nav` element, no
    `aria-label="Breadcrumb"`, and no `role` semantics. Screen readers
    announce it as a generic group of spans. Each segment is a static `<span>`
    (line 37) — VS Code-style click-to-navigate-to-folder is not implemented,
    so the breadcrumb is purely decorative. That is acceptable as a v1, but
    the lack of an `aria-label` means AT users don't even know this region
    represents the current file's path. Cheap fix: wrap in a `<nav
    aria-label="Breadcrumb">` (or add `role="navigation" aria-label="Breadcrumb"`
    to the existing div). If/when segments become clickable, each should be a
    `<button>` with an accessible name. Also note the container uses
    `overflow-hidden text-ellipsis whitespace-nowrap` (line 27) but the
    ellipsis is applied to the flex container, not to a single truncating
    child — long paths will simply clip on the right without a visible
    ellipsis glyph because each segment truncates independently (line 39
    `truncate` is per-segment, so the *last* segment shows an ellipsis but
    middle segments don't). Minor visual issue.
- verification: |
    Read BreadcrumbBar.tsx lines 23-49: the container is a `<div>` with no
    ARIA label; segments are `<span>`s with no interactivity. The `truncate`
    class on line 39 is applied per-segment, not to the whole path.
