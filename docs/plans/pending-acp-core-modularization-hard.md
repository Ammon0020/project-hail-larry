# Epic: ACP Core Modularization

> **Status:** pending. **Owner:** —. **Created:** 2026-07-22.
> **Difficulty:** hard. **Depends on:** completed `S-ACP-CORE-session-transport`.
> **Related:** `src/acp/core.rs`, `src/acp/{providers,context,store,profile}.rs`,
> `src/mcp/mod.rs`, `cmd/mockagent/`. Stories: `docs/plans/acp-core-modularization/`.

## Goal

Split `src/acp/core.rs` into cohesive internal modules without changing the
`ACPClient` contract, process-security guarantees, or ACP transport behavior.
Leave `Client` as the stable facade and make ownership, callback policy, and
session persistence independently understandable and testable.

## Architecture decisions (locked)

- One session actor task exclusively owns `ConnectionTo<Agent>`. No extracted
  service may retain or share that connection.
- Extract under `src/acp/core/` with `core.rs` becoming a thin facade. Use
  `pub(super)` by default; do not expand `src/acp`'s public API for this work.
- Preserve the actor readiness/registry-publication handshake, ordered durable
  session-update writes, per-session cancellation, callback semaphore, path
  containment, and process-group cleanup exactly.
- Move focused unit tests with their implementation. Retain cross-module actor
  tests for prompt cancellation, close, rebind, dormant restore, and startup.
- Do not add an ACP proxy, HTTP transport, or runtime MCP broker as part of
  modularization.

## Story Index

| ID | Story | Difficulty | Depends on | Status |
|----|-------|------------|------------|--------|
| S-ACP-MOD-BASE | [Baseline invariants and module seams](acp-core-modularization/pending-baseline-small.md) | small | — | pending |
| S-ACP-MOD-CALLBACKS | [Extract callback and local-tool handlers](acp-core-modularization/pending-callback-handlers-med.md) | med | BASE | pending |
| S-ACP-MOD-ACTOR | [Extract agent process and SDK connection setup](acp-core-modularization/pending-actor-startup-med.md) | med | BASE | pending |
| S-ACP-MOD-TURN | [Extract single-owner actor turn state machine](acp-core-modularization/pending-turn-state-machine-hard.md) | hard | CALLBACKS, ACTOR | pending |
| S-ACP-MOD-REGISTRY | [Isolate session registry and durable lifecycle](acp-core-modularization/pending-session-registry-med.md) | med | TURN | pending |
| S-ACP-MOD-VERIFY | [Validate SDK boundaries and diagnostic tooling](acp-core-modularization/pending-sdk-verification-small.md) | small | REGISTRY | pending |

**Suggested sequence:** BASE → (CALLBACKS ∥ ACTOR) → TURN → REGISTRY → VERIFY.

## Module destination

```text
src/acp/core.rs                 facade and private module declarations
src/acp/core/registry.rs        Client facade, session metadata, persistence, restore
src/acp/core/actor.rs           child spawn, stderr, SDK builder, init/new/load
src/acp/core/turn.rs            actor commands, prompt/control loop, cancellation
src/acp/core/handlers/          notifications, callback capacity, fs/permission/terminal
src/acp/core/mcp.rs             profile-filtered session MCP attachment
src/acp/core/diagnostics.rs     bounded/redacted agent stderr
```

## SDK decisions

- Continue with `agent-client-protocol`; its typed builders and handlers belong
  behind `actor.rs`, not in the session registry.
- `agent-client-protocol-http` is deferred: local child agents use stdio and
  require explicit cwd, stderr-tail, and process-group lifecycle control.
- `agent-client-protocol-rmcp` is deferred unless this daemon embeds or serves
  MCP servers; current `mcp.json` conversion attaches ordinary servers.
- Keep native MCP-over-ACP capability-gated and opt-in. It is unrelated to the
  split and must not require a proxy deployment.
- `conductor` and `polyfill` are deferred; they address proxy chains and
  compatibility bridges, not the present local-client topology.
- Use trace viewer, cookbook, yopo, and protocol test utilities only as
  development/test aids after the core split proves a need.

## Scope

**In scope:** internal Rust module moves, narrow internal interfaces, test
relocation/additions, and a post-split SDK diagnostic/test-tool decision.

**Out of scope:** externally observable REST/WS changes, agent capability
changes, a daemon-hosted ACP service, MCP broker implementation, changing
permission policy, or replacing `async-process` child management.

## Completion criteria

- `core.rs` is a small facade; named destination modules own their concerns.
- The actor remains the sole owner of `ConnectionTo<Agent>`.
- Existing security/lifecycle invariants have direct regression coverage.
- Relevant Rust checks pass; any unrelated failures are recorded honestly.
