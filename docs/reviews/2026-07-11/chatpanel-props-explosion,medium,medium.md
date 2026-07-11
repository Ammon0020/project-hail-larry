# ChatPanel Props Explosion (25+ Props)

## Location
- [ChatPanel.tsx:44-103](file:///media/adam/extex/projects/project-hail-larry/web/src/components/ChatPanel.tsx#L44-L103) — prop type definition
- [App.tsx:773-825](file:///media/adam/extex/projects/project-hail-larry/web/src/App.tsx#L773-L825) — call site

## Problem

`ChatPanel` accepts **25+ props**, many of which are callbacks that simply forward to `backend.*` methods:

```tsx
onSendMessage, onCreateSession, onPermissionResponse, onSelectSession,
onCancel, onRenameSession, onDeleteSession, onRebindSession,
onSwitchModel, onExportSession, onUploadFile, ...
```

This creates a "prop drilling" pattern where `App.tsx` acts as a manual wire-board, threading every backend action through itself into ChatPanel, which then threads subsets into `ChatComposer`, `ConversationView`, `ChatTabBar`, etc.

## Impact

- Adding a new backend action requires modifying 3+ files (api.ts → useBackend → App.tsx → ChatPanel → sub-component).
- The `App.tsx` call site for `<ChatPanel>` spans 50+ lines of props.
- Type safety is weakened because the callback signatures are repeated in each component's prop type.

## Suggested Fix

Use React context to provide backend actions:

```tsx
// BackendContext.tsx
const BackendContext = createContext<ReturnType<typeof useBackend>>(...)

// App.tsx
<BackendContext.Provider value={backend}>
  <ChatPanel activeSessionId={...} />
</BackendContext.Provider>

// ChatPanel.tsx (or any sub-component)
const { sendPrompt, cancelSession } = useContext(BackendContext)
```

This eliminates prop-drilling for action callbacks. Components still receive data props (events, sessions, agents) as needed.
