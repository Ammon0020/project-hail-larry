# configs/

## Responsibility

Shipped runtime configuration files bundled with the daemon binary.

## Module Map

```text
configs/
└── system-messages.json   default agent-session messages
```

## Rules & Patterns

- **Immutable Defaults**: Treat bundled configs as immutable default settings; user overrides live in `~/.local-agent`.
- **Schema Validation**: Validate any changes against Rust structs in `src/config/`.
- **No Secrets**: Never commit environment-specific credentials or secrets into default config templates.
