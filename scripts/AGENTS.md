# scripts/

## Responsibility

Developer setup scripts, environment guards, and smoke test utilities.

## Module Map

- **`setup.sh`** / **`setup.ps1`** — Environment bootstrap and developer dependency installers.
- **`spa-smoke.sh`** / **`spa-smoke.ps1`** — Single-page app build and smoke test scripts.
- **`exec-guard.py`** — Shell execution guard helper and policy filter.

## Rules & Patterns

- **Idempotency**: All setup and utility scripts must be idempotent and safe to run multiple times.
- **Cross-Platform**: Maintain parity between bash (`.sh`) and PowerShell (`.ps1`) variants.
- **Make First**: Prefer invoking `make` targets or `cargo` commands over writing custom one-off scripts.
