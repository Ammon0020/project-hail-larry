# Story S-ACP-SPIKE: ACP SDK Proof of Capability

> **Phase:** 0 | **Depends on:** S-ARCH | **Go source:** `internal/acp/`, `cmd/mockagent/`

## Goal

Prove the current official Rust ACP SDK supports the critical client behavior
before other Rust services rely on it. This is a disposable, narrowly scoped
integration proof; its verified API shape becomes the input to ACP stories.

## Scope

- Keep `cmd/mockagent/` in Go as the deterministic ACP fixture
- Use the current official Rust SDK process/stdio helpers where available
- Start a mock agent and one locally available real ACP agent when configured
- Verify initialize, session lifecycle, streaming, file/shell callbacks,
  permission request/response, cancellation, PKCE/auth shape, and MCP relay
  capability
- Record exact crate versions, MSRV, unsupported protocol operations, and any
  retained workaround

## Acceptance Criteria

- [ ] Mock-agent CI test covers initialize, create, prompt, stream, cancel, and close
- [ ] File write/read and shell callback round trips are verified
- [ ] Permission request is delivered and first response wins
- [ ] Child process and its process tree terminate on cancellation
- [ ] A configured real agent completes an opt-in E2E prompt round trip
- [ ] MCP relay availability is proven; absence has an isolated fallback design
- [ ] The plan no longer relies on unverified SDK examples or guessed APIs
