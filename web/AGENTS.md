# web/

## Responsibility

Vite + React web client served by the Rust daemon.
- Entry: `web/src/main.tsx`
- Build: `web/vite.config.ts`, `web/package.json`

## Module Map

```text
web/
├── src/
│   ├── App.tsx           app shell/composition
│   ├── components/      UI (See src/components/AGENTS.md)
│   │   ├── chat/         chat/thread/tool parts
│   │   ├── assistant-ui/ assistant primitives
│   │   ├── git/          source control UI
│   │   ├── preview/      preview renderers
│   │   ├── settings/     settings panels
│   │   └── ui/           Radix primitives
│   ├── hooks/            backend and UI state
│   ├── lib/              API client/utilities
│   └── types/            Rust-wire mirrors
├── public/               static files
└── vite.config.ts        build config
```

## Rules & Patterns

- Co-locate utilities with components; use semantic tokens and `cva` for stable variants.
- Use `cn` for conditional classes. Reserve CSS/`@apply` for global or third-party needs.
- Design mobile-first; use `dark:` or `data-theme` classes, never JS theme conditionals.
- UI never talks directly to an agent implementation; all actions route through the daemon API.
