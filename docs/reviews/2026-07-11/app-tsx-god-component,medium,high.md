# App.tsx Is a God Component (841 Lines)

## Location
- [App.tsx](file:///media/adam/extex/projects/project-hail-larry/web/src/App.tsx) — entire file (841 lines)

## Problem

`App.tsx` at **841 lines** is the largest frontend file and concentrates responsibilities that should be separate:

1. **Tab management** — `openTabs`, `activeTabId`, `handleTabSelect`, `handleTabClose`, `handleContentChange`, `handleSave`, `handleReloadTab`, `openSettingsTab` (≈ 100 lines)
2. **File-change detection** — `processedFileEventIdRef` effect + silent-refresh logic (≈ 50 lines)
3. **Session management** — `activeSessionId`, `handleCreateSession`, `handleSelectSession`, `handleSendMessage`, `loadedSessionRef`, session-validation effect (≈ 60 lines)
4. **Panel layout & persistence** — `leftPanelWidth`, `rightPanelWidth`, `startPanelDrag`, resize handlers (≈ 80 lines)
5. **Global keyboard shortcuts** — `Ctrl+S`, `Ctrl+W`, `Ctrl+B`, `Ctrl+Shift+F/E` (≈ 50 lines)
6. **Search result navigation** — `searchResultLine`, `handleSearchResultSelect` (≈ 25 lines)
7. **Backend ↔ UI prop threading** — ~40 props passed to `<ChatPanel>` alone

The component has **14 `useEffect` hooks** and **13 `useState` declarations** in a single function body.

## Impact

- Any change to tab logic risks breaking session management or panel layout because they share the same render scope.
- The 40-prop `<ChatPanel>` call site is a maintenance burden — adding any new capability requires threading through App.

## Suggested Fix

Extract custom hooks:
- `useTabManager(backend)` → `openTabs`, `activeTabId`, tab CRUD, save, reload
- `usePanelLayout()` → widths, drag, `Ctrl+B` toggle
- `useFileChangeDetection(backend, openTabs)` → the file-event effect
- `useGlobalShortcuts(tabManager, panelLayout)` → keyboard handler
- `useSessionManager(backend)` → session CRUD, validation, loaded tracking

Each hook owns its own state + effects. App.tsx becomes a ~200-line composition.
