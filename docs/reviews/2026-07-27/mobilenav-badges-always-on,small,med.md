- name: MobileNav badges are hardcoded and always visible, misleading the user
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/MobileNav.tsx
- lines: 28-32, 45-47
- description: |
    The Editor nav item always shows a `w-1.5 h-1.5 bg-primary` dot (line 30)
    and the Chat item always shows a `w-2 h-2 bg-primary animate-pulse` dot
    (line 31). These badges are not tied to any state — the Editor dot
    presumably is meant to indicate unsaved changes, and the Chat dot is meant
    to indicate new agent activity / pending permission prompts. Because they
    are always on, the user learns to ignore them, which defeats the purpose
    and means real notifications will be missed. The pulsing chat badge in
    particular draws the eye permanently for no reason. The badges should be
    driven by props: `hasUnsavedEdits` for Editor and `hasPendingActivity`
    (pending permissions or new streaming events) for Chat, both of which are
    already available in App.tsx (`openTabs.some(t => t.unsaved)` and
    `backend.pendingPermissions.length > 0` / session streaming state).
- verification: |
    Read MobileNav.tsx lines 28-32: `badge` is a static className string on
    both items, never conditional. Lines 45-47 render the badge div whenever
    `badge` is truthy. The component receives no `unsavedCount` or
    `pendingCount` prop (lines 12-22), so there is no way for it to reflect
    real state.
