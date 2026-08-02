# scripts/

## Responsibility

Developer setup scripts, environment guards, and smoke test utilities.

## Module Map

```text
scripts/
├── setup.sh, setup.ps1       environment bootstrap
├── spa-smoke.sh, spa-smoke.ps1 SPA build/smoke tests
└── exec-guard.py              shell policy guard
```

## Rules & Patterns

- **Idempotency**: All setup and utility scripts must be idempotent and safe to run multiple times.
- **Cross-Platform**: Maintain parity between bash (`.sh`) and PowerShell (`.ps1`) variants.
- **Make First**: Prefer invoking `make` targets or `cargo` commands over writing custom one-off scripts.
