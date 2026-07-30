# tests/

## Responsibility

Test infrastructure and contract suites.

## Module Map

- `contract/` — Source-of-truth contract fixtures, golden files, scripts.
- **`contract_runner/`** — Integration test harness for REST/WS validation and dynamic redaction. (See `tests/contract_runner/AGENTS.md`)
- `migrate/` — Config migration tests.

## Rules & Patterns

- Contract tests define expected wire behavior; keep golden files authoritative.
- Unrelated failures are recorded in `docs/known-issues.md`; do not expand scope to fix them.
- Prefer `cargo test -q --all-targets` for Rust and the contract runner for integration gates.
