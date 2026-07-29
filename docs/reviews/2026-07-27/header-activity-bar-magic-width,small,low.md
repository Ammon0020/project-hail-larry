- name: Header width couples to ActivityBar width via magic number 48
- file: /media/adam/extex/projects/project-hail-larry/web/src/App.tsx
- lines: 953
- description: |
    The desktop header's left section sets `style={{ width: leftPanelWidth + 48 }}`
    (line 953) so it lines up with the ActivityBar (w-12 = 48px) plus the
    LeftSidebar. The `48` is a hardcoded duplicate of the `w-12` class on
    ActivityBar.tsx line 26. If the activity bar width ever changes (e.g. a
    design token bump to w-14), the header column and the sidebar below it
    silently drift out of alignment, producing a visible vertical seam. This
    is fragile implicit coupling between two files with no shared constant.
    Recommend extracting a `ACTIVITY_BAR_WIDTH = 48` constant (or a CSS
    variable / design token) imported by both components, or driving the
    header layout with the same flex structure as the row below it so no
    arithmetic is needed. Minor visual-correctness issue but easy to prevent.
- verification: |
    Read App.tsx line 953: `style={{ width: leftPanelWidth + 48 }}` with no
    comment linking 48 to ActivityBar. Read ActivityBar.tsx line 26:
    `className="… w-12 …"` (Tailwind w-12 = 3rem = 48px). No shared constant
    between the two files.
