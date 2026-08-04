# tests/

## Responsibility

Test infrastructure and contract suites.

## Module Map

```text
tests/
├── contract/          fixtures, goldens, scripts
├── contract_runner/   REST/WS harness (See contract_runner/AGENTS.md)
├── migrate/           config migration tests
├── acp_core_lifecycle.rs
├── spike_acp.rs
└── ...                integration tests
```

## Rules & Patterns

- Contract tests define expected wire behavior; keep golden files authoritative.
- Unrelated failures are recorded in `docs/known-issues.md`; do not expand scope to fix them.
- Prefer `cargo test -q --all-targets` for Rust and the contract runner for integration gates.
