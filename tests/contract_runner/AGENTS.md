# tests/contract_runner/

## Responsibility

Integration contract test runner binary. Executes recorded HTTP REST and WebSocket conversation transcripts against running daemon instances to verify protocol compliance.

## Module Map

```text
tests/contract_runner/
├── main.rs       runner/CLI/fixture loop
├── harness.rs    daemon lifecycle/isolation
├── rest.rs       REST requests/assertions
├── ws.rs         WebSocket frames/assertions
├── compare.rs    canonical output diffs
├── dto.rs        transcript/response types
└── redactor.rs   volatile/secret redaction
```

## Rules & Patterns

- **Deterministic Golden Files**: Redact all transient data (UUIDs, timestamps, ephemeral ports, file paths) to ensure deterministic test passes across environments.
- **Contract Integrity**: Do not edit golden expectation files to make broken code pass; investigate protocol mismatch root causes.
- **Isolation**: Each test run must launch a clean, isolated daemon instance with ephemeral storage to avoid cross-test pollution.
