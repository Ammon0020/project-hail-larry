# web/src/components/

## Responsibility

React components and view containers powering the Web IDE user interface, editor panes, sidebars, and chat panels.

## Module Map

```text
web/src/components/
├── ActivityBar.tsx, LeftSidebar.tsx       navigation/panels
├── WorkspaceHeader.tsx, MobileNav.tsx     workspace/mobile chrome
├── BreadcrumbBar.tsx, StatusBar.tsx       path/status chrome
├── FileTree.tsx                            explorer/tree actions
├── FileViewer.tsx, EditorPane.tsx          editor/viewers
├── TabBar.tsx, tabPreviewState.ts           tabs/previews
├── SearchPanel.tsx                          code search
├── chat/                                    chat/composer/thread/tools
├── assistant-ui/                            assistant primitives
├── git/                                     source control UI
├── preview/                                 preview renderers
├── settings/                                settings panels
├── ui/                                      Radix primitives
├── BrowsePreview.tsx, SwitchAgentDialog.tsx preview/agent dialogs
└── CommandPalette.tsx, LockScreen.tsx       commands/pairing
```

## Rules & Patterns

- **Styling**: Use semantic tokens, Tailwind utilities, and `cva` for component variants. Combine conditional classes using `cn(...)`.
- **Theme**: Mobile-first design; use Tailwind `dark:` variant or `data-theme` attribute selectors. Never read JS theme state for CSS conditionals.
- **Co-location**: Keep component-specific state and small helper hooks co-located with their parent component.
- **Backend Isolation**: UI components must never instantiate or talk to agent models directly; all interactions route through backend hooks (`useBackend`).
