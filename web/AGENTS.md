# web/

## Responsibility

Vite + React web client served by the Rust daemon.
- Entry: `web/src/main.tsx`
- Build: `web/vite.config.ts`, `web/package.json`

## Module Map

- **`src/components/`** — React components, panels, view widgets, and UI primitives. (See `web/src/components/AGENTS.md`)
  - `assistant-ui/` — Assistant UI primitives.
  - `chat/` — Chat panel, composer, thread, tool parts.
  - `git/` — Git diff viewer and source control panel.
  - `preview/` — File/preview renderers.
  - `settings/` — Settings panels.
  - `ui/` — Shared reusable UI (Radix primitives).
- `src/hooks/` — React hooks: `useBackend`, `useChatTabs`, `useEditorSettings`, `useTheme`, etc.
- `src/lib/` — API client, errors, model prefs, theme, utilities.
- `src/types/` — TypeScript mirrors of Rust `src/interfaces/`.
- `src/assets/`, `public/` — Static assets and HTML entry.

## Rules & Patterns

- Co-locate utilities with components; use semantic tokens and `cva` for stable variants.
- Use `cn` for conditional classes. Reserve CSS/`@apply` for global or third-party needs.
- Design mobile-first; use `dark:` or `data-theme` classes, never JS theme conditionals.
- UI never talks directly to an agent implementation; all actions route through the daemon API.
