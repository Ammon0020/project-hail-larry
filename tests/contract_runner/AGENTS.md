# tests/contract_runner/

## Responsibility

Integration contract test runner binary. Executes recorded HTTP REST and WebSocket conversation transcripts against running daemon instances to verify protocol compliance.

## Module Map

- **`main.rs`** — Test runner entry point, CLI arguments parsing, fixture discovery, and test execution loop.
- **`harness.rs`** — Test harness lifecycle management, temporary directory creation, daemon binary execution, and environment setup.
- **`rest.rs`** — REST request builder, HTTP assertion executor, and status code verification.
- **`ws.rs`** — WebSocket client listener, message frame deserializer, and streaming event assertion logic.
- **`compare.rs`** — Canonical JSON and output diff comparison engine for golden file matching.
- **`dto.rs`** — Data Transfer Objects representing transcript inputs, recorded responses, and assertion schemas.
- **`redactor.rs`** — Sensitive and volatile field redactor (scrubs timestamps, session IDs, tokens, and local paths before comparison).

## Rules & Patterns

- **Deterministic Golden Files**: Redact all transient data (UUIDs, timestamps, ephemeral ports, file paths) to ensure deterministic test passes across environments.
- **Contract Integrity**: Do not edit golden expectation files to make broken code pass; investigate protocol mismatch root causes.
- **Isolation**: Each test run must launch a clean, isolated daemon instance with ephemeral storage to avoid cross-test pollution.
