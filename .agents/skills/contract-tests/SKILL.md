# Contract Differential Tests

## When to use

Run these tests when:
- You've changed the Rust backend's REST API, WebSocket behavior, or DTO shapes
- Before committing changes to API-facing code (`src/server/`, `src/acp/`, `src/config/`, `src/interfaces/`, etc.)
- The golden fixtures are checked in and static; there is no live regeneration step (the original Go harness has been removed)

## What it does

The contract runner (`tests/contract_runner/`) is a Rust `cargo test` integration test that boots the Rust backend binary as a subprocess, replays HTTP/WS request sequences from the golden fixtures, applies the same redactions originally used by the (now-removed) Go harness, and compares responses. It tests the **external API contract** — not internal implementation details.

## How to run

```sh
# Against the Rust backend (default via make):
make test-contract

# Or directly (note the --features contract flag — the runner is feature-gated):
cargo test --test contract_runner --features contract -- --nocapture

# Use a pre-built binary:
CONTRACT_BINARY=/path/to/local_agent cargo test --test contract_runner --features contract

# Keep the state dir for debugging:
CONTRACT_KEEP_STATE=1 cargo test --test contract_runner --features contract

# Run a single test:
cargo test --test contract_runner --features contract rest_health_ok -- --nocapture

# Run a category (REST, WS, DTO):
cargo test --test contract_runner --features contract rest_ -- --nocapture
cargo test --test contract_runner --features contract ws_ -- --nocapture
cargo test --test contract_runner --features contract dto_ -- --nocapture
```

## What is tested

- **REST** (45 tests): every `golden/rest/*.json` fixture replayed as HTTP. Semantic JSON comparison for object/array bodies, exact byte comparison for error text and non-JSON. Envelope (method, path, status, contentType) always exact.
- **WebSocket** (5 tests): origin rejection (403), connection success (101 + ping/pong), live broadcast (API-driven), `?after=` replay + live, auth rejection (401 via non-loopback).
- **DTO** (3 tests): structural field name/type comparison against `golden/dto/*.json` with omitempty tolerance.
- **Unit tests** (14): redactor and compare utilities.

## What is NOT tested

- **CLI commands** — the CLI is a thin client over the REST API. Its output formatting (box-drawing, tables, help text) is presentation, not contract. The checked-in `golden/cli/` fixtures are historical documentation captured by the original Go harness; the runner doesn't test them.
- **`rest_agents_autodetect_ok`** — `#[ignore]` because autodetect results are machine-specific. The runner neutralizes autodetect (`PATH=/dev/null`, `HOME=/dev/null`) for reproducibility.
- **`rest_mcp_put_bad_body`** — `#[ignore]`; the original Go `encoding/json` vs Rust `serde_json` parse-error text differed.
- **WS slow-client recovery** — black-box infeasible; unit-tested in `src/sync/tests.rs`.

## Important: gated behind `contract` feature

The contract runner is feature-gated (`#![cfg(feature = "contract")]`). Without `--features contract`, the test binary compiles to nothing — no tests, no dependencies pulled in, no backend subprocess. This keeps `cargo test --all-targets` (and the main CI `test` job) fast and dependency-free.

Always run it explicitly via `make test-contract` or `cargo test --test contract_runner --features contract`.

**CI:** Linux job `contract` in `.github/workflows/rust-ci.yml` (after `test`) builds
`./target/debug/local_agent` then runs the suite with `--test-threads=1`.

## Regenerating golden fixtures

The golden fixtures are checked in and static — they are the contract surface.
The original Go harness that captured them has been removed, so there is no
live regeneration step. If an intentional API change requires new fixtures,
re-adding a Go capture harness is out of scope; update the goldens by hand or
reintroduce a capture tool deliberately.

## Files

- `tests/contract_runner/main.rs` — test entry point, all `#[tokio::test]` functions
- `tests/contract_runner/harness.rs` — backend process management (build, start, health check, shutdown)
- `tests/contract_runner/redactor.rs` — redaction logic (Rust port of the original `go-fixtures/redact.go`)
- `tests/contract_runner/compare.rs` — JSON comparison utilities (semantic + exact)
- `tests/contract_runner/rest.rs` — REST test cases and runner
- `tests/contract_runner/ws.rs` — WebSocket tests
- `tests/contract_runner/dto.rs` — DTO shape comparison tests
- `tests/contract/golden/` — checked-in golden fixtures (the contract surface)
- `tests/contract/README.md` — full documentation
