# web/src/components/

## Responsibility

React components and view containers powering the Web IDE user interface, editor panes, sidebars, and chat panels.

## Module Map

### Layout & Navigation
- **`ActivityBar.tsx`** — Leftmost navigation bar for switching primary view modes (explorer, search, git, settings).
- **`LeftSidebar.tsx`** — Resizable collapsible container hosting explorer, search, and git sub-panels.
- **`BreadcrumbBar.tsx`** / **`StatusBar.tsx`** — Top path navigation breadcrumbs and bottom status bar (connection status, active mode, line/col info).
- **`MobileNav.tsx`** — Bottom tab navigation bar optimized for mobile responsive layouts.
- **`WorkspaceHeader.tsx`** — Workspace title header with pairing indicator and workspace switching.

### Editor & Explorer
- **`FileTree.tsx`** — Interactive directory tree with file creation, deletion, rename, and selection handlers.
- **`FileViewer.tsx`** / **`EditorPane.tsx`** — Code editor container with tab management, syntax highlighting, and live revision syncing.
- **`TabBar.tsx`** / **`tabPreviewState.ts`** — Tab strip for open file editors and active preview tabs.
- **`SearchPanel.tsx`** — Code search input pane interfacing with backend ripgrep search engine.

### AI Assistant & Chat
- **`chat/`** — Core chat components: `ChatPanel.tsx`, `ChatComposer.tsx`, `ChatHistory.tsx`, `ChatTabBar.tsx`, tool call renders, and message stream parts.
- **`assistant-ui/`** — Primitive components for AI turn rendering, tool execution approval dialogs, and stream status indicators.
- **`SwitchAgentDialog.tsx`** — Modal for switching active agent harness, model, or provider options.

### Auxiliary Panels & Dialogs
- **`git/`** — Git panel, branch picker, diff viewer, and commit controls.
- **`preview/`** / **`BrowsePreview.tsx`** — Embedded browser preview renderer for running web servers/APIs.
- **`settings/`** / **`SettingsPanel.tsx`** / **`ProfilesSettings.tsx`** — Daemon settings, keybinding preferences, and agent profile options.
- **`CommandPalette.tsx`** — Quick keyboard command launcher (`Ctrl+Shift+P` / `Cmd+Shift+P`).
- **`LockScreen.tsx`** — Mnemonic authentication overlay for unpaired devices.
- **`ui/`** — Reusable primitives built on Radix UI (`dialog`, `dropdown-menu`, `button`, `tooltip`, `scroll-area`).

## Rules & Patterns

- **Styling**: Use semantic tokens, Tailwind utilities, and `cva` for component variants. Combine conditional classes using `cn(...)`.
- **Theme**: Mobile-first design; use Tailwind `dark:` variant or `data-theme` attribute selectors. Never read JS theme state for CSS conditionals.
- **Co-location**: Keep component-specific state and small helper hooks co-located with their parent component.
- **Backend Isolation**: UI components must never instantiate or talk to agent models directly; all interactions route through backend hooks (`useBackend`).
