- name: Tree operations use native prompt/alert/confirm — bad UX, blocked in some webviews
- file: /media/adam/extex/projects/project-hail-larry/web/src/App.tsx
- lines: 798, 806, 812, 819, 514, 524, 540
- description: |
    The file-tree handlers fall back to browser-native dialogs:
      * `handleTreeNewFile` — `window.prompt('New file name')` (line 798),
        `window.alert(...)` on error (line 806).
      * `handleTreeNewFolder` — `window.prompt('New folder name')` (line 812),
        `window.alert(...)` on error (line 819).
      * `handleTreeRename` — `window.alert(...)` on error (line 514). (The
        rename input itself is handled in FileTree, not here.)
      * `handleTreeDelete` — `window.confirm(...)` (line 524),
        `window.alert(...)` on error (line 540).
    Native dialogs are unstyled (clash with the app theme), non-composable,
    tab-trapping, and are actively blocked by some embedded webviews / sandbox
    iframes. They also cannot show validation messages (e.g. "name contains a
    path separator") without a second alert. The delete confirm message on
    line 523 uses the full `path` for folders (`Delete folder "src/foo"?`)
    rather than the folder's display name, which is confusing. Recommend a
    small themed modal component for prompt/confirm/alert reused across these
    handlers, with proper focus management and an inline error slot. This is a
    larger refactor but the current state is a consistent UX sore spot in an
    otherwise polished IDE surface.
- verification: |
    Read App.tsx lines 797-808, 811-821, 487-518, 521-544 — all four tree
    handlers use `window.prompt` / `window.alert` / `window.confirm`. No
    themed modal equivalent exists in `web/src/components/ui/` for these
    primitives (Banner exists for banners, not for prompts).
