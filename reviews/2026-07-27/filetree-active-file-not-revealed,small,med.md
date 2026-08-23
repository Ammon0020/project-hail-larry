- name: Active file's parent folders are not auto-expanded in the tree
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/FileTree.tsx
- lines: 502-532, 369
- description: |
    When a file becomes active (`node.active === true`, line 369), the tree
    highlights its row but does **not** expand the parent folders leading to
    it. If the user opens a file from search results, chat, or a breadcrumb
    action, the file is active but its row is invisible because the enclosing
    folders are collapsed (and `expandedPaths` only changes via explicit
    `onToggleExpand`, line 521-532).

    User impact: the user opens a file, sees it in the editor, but the tree
    gives no visual indication of where it is — they have to manually drill
    down to find it. This breaks the expected IDE affordance where the explorer
    reveals the active file.

    Suggested fix: when `nodes` change or an `active` flag flips on a file,
    compute the set of ancestor folder paths for the active node and merge them
    into `expandedPaths` (without removing user-collapsed folders). This can
    be done in the FileTree component by walking the node tree to find the
    active file's ancestor paths, or by accepting an `activeFilePath` prop and
    expanding its prefix.
- verification: |
    Read FileTree.tsx lines 502-532 (expandedPaths state + handleToggleExpand)
    and line 369 (active row styling). `expandedPaths` is only mutated by
    `handleToggleExpand`, `ensureExpanded` (after new-file/folder creation),
    and workspace reset. There is no logic that expands ancestors of the
    active file. The `node.active` flag is consumed only for row styling.
