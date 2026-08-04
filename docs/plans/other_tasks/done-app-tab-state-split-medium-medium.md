# App tab state split

> Difficulty: medium. Urgency: medium. Status: pending.

## Goal

Reduce `web/src/App.tsx` by extracting pure path/tab logic and cohesive tab
state while keeping App as the layout composition root.

## Scope

```text
web/src/
├── App.tsx                 layout/composition root
├── lib/tabPath.ts          pure path/tab helpers
└── hooks/
    ├── useTabManager.ts   tab state, persistence, close/remap handlers
    ├── useLayoutState.ts  panel/mobile/settings state
    └── useEditorTabHandlers.ts  diff/preview/search openers
```

Extract state together with the effects that write it. Preserve the file-select
race guard, rename/delete tab remapping, localStorage keys, and mobile/desktop
layout behavior. Do not create a prop-heavy `AppLayout.tsx` solely to reduce
line count.

## Acceptance

- App remains the composition root with no behavior or navigation changes.
- Tab persistence, rename/delete remapping, preview tabs, and responsive layout
  behave identically.
- New hooks/helpers are independently testable where practical.

## Verification

```text
npm run lint --silent
npm run build --silent
make check
```
