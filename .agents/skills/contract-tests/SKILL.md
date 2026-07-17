# Contract Differential Tests

## When to use

Run these tests when:
- You've changed the Go backend's REST API, WebSocket behavior, or DTO shapes
- You're working on the Rust port and want to verify API equivalence
- You've regenerated the golden fixtures (`go test ./tests/contract/go-fixtures/ -run TestGenerateFixtures`)
- Before committing changes to `internal/server/`, `internal/acp/`, `internal/config/`, `internal/interfaces/`, or any API-facing code

## What it does

The contract runner (`tests/contract_runner/`) is a Rust `cargo test` integration test that boots a backend binary (Go or Rust) as a subprocess, replays HTTP/WS request sequences from the golden fixtures, applies the same redactions as the Go harness, and compares responses. It tests the **external API contract** — not internal implementation details.

## How to run

```sh
# Against the Go backend (default — builds go binary, boots it as subprocess):
make test-contract

# Or directly (note the --features contract flag — the runner is feature-gated):
cargo test --test contract_runner --features contract -- --nocapture

# Against the Rust backend:
CONTRACT_BACKEND=rust cargo test --test contract_runner --features contract -- --nocapture

# Use a pre-built binary:
CONTRACT_BINARY=/path/to/local-agent cargo test --test contract_runner --features contract

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
- **WebSocket** (2 tests): origin rejection (403 for cross-origin) and connection success (101 + ping/pong).
- **DTO** (3 tests): structural field name/type comparison against `golden/dto/*.json` with omitempty tolerance.
- **Unit tests** (14): redactor and compare utilities.

## What is NOT tested

- **CLI commands** — the CLI is a thin client over the REST API. Its output formatting (box-drawing, tables, help text) is presentation, not contract. The Go harness still captures CLI fixtures for documentation.
- **`rest_agents_autodetect_ok`** — `#[ignore]` because autodetect results are machine-specific. The runner neutralizes autodetect (`PATH=/dev/null`, `HOME=/dev/null`) for reproducibility.
- **WS auth rejection** — requires non-loopback TCP, can't be tested black-box.
- **WS event broadcast** — requires in-process event triggering, can't be done black-box.

## Important: gated behind `contract` feature

The contract runner is feature-gated (`#![cfg(feature = "contract")]`). Without
`--features contract`, the test binary compiles to nothing — no tests, no
dependencies pulled in, no Go subprocess. This keeps `cargo test --all-targets`
(and CI) fast and dependency-free.

Always run it explicitly via `make test-contract` or
`cargo test --test contract_runner --features contract`.

## Regenerating golden fixtures

If you've intentionally changed the API and the contract tests fail, regenerate the golden fixtures:

```sh
go test ./tests/contract/go-fixtures/ -run TestGenerateFixtures
```

Then commit the updated `tests/contract/golden/` files. The contract runner will compare against the new fixtures.

## Files

- `tests/contract_runner/main.rs` — test entry point, all `#[tokio::test]` functions
- `tests/contract_runner/harness.rs` — backend process management (build, start, health check, shutdown)
- `tests/contract_runner/redactor.rs` — redaction logic (ports Go's `go-fixtures/redact.go`)
- `tests/contract_runner/compare.rs` — JSON comparison utilities (semantic + exact)
- `tests/contract_runner/rest.rs` — REST test cases and runner
- `tests/contract_runner/ws.rs` — WebSocket tests
- `tests/contract_runner/dto.rs` — DTO shape comparison tests
- `tests/contract/golden/` — checked-in golden fixtures (the contract surface)
- `tests/contract/go-fixtures/` — Go harness that generates the golden fixtures
- `tests/contract/README.md` — full documentation
