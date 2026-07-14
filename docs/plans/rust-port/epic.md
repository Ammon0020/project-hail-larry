# Epic: Convert Go Backend to Rust

> **Status:** Planning hardening. **Owner:** —. **Estimate:** Moderate-hard; implementation starts only after Phase 0 gates pass.
> Library references: `docs/rust-ecosystem/`. Stories: `stories/`.

## Goal

Replace the Go daemon (`cmd/app/` + `internal/`) with a Rust implementation
while preserving the React frontend, REST API, WebSocket protocol, CLI
behavior, and upgradeability of existing `~/.local-agent/` state. Internal
architecture may improve when it is protected by explicit wire and migration
compatibility tests.

## Scope

**In scope:**
- All current backend capabilities, CLI commands, and platform services
- Contract fixtures for REST, WebSocket, CLI, and serialized shared types
- A safe migration/read-compatibility path for existing user state
- Rust-native internal boundaries: typed errors/events, constructor-complete
  wiring, per-session cancellation, and ordered event persistence/publication

**Out of scope:**
- Frontend feature changes or wire-protocol redesign
- A persistence-format redesign during the initial port
- New product capabilities

## Architecture Decisions

- **Single Cargo package, focused modules.** Start with `src/{app,acp,api,
  config,events,files,...}`. Extract crates only for a proven dependency cycle,
  external reuse, or materially harmful compile time.
- **Compatibility before consolidation.** Preserve existing config, SQLite,
  uploads, and JSON file semantics first. Consolidating stores into SQLite is a
  separately planned post-port migration, not an implicit rewrite change.
- **Constructor-only wiring.** Services receive narrow dependencies at
  construction; no post-construction `Set*` callbacks. Commands use direct
  typed service dependencies. A narrow event publisher is only for persisted
  app events and subscriber notification, not a general command bus.
- **Stable wire adapter.** Rust uses typed internal event/error models, but a
  dedicated adapter must produce the existing JSON shapes exactly.
- **Event ordering.** Persist an event before publishing it. Reconnection
  subscribes, replays from the durable cursor, then deduplicates by event ID
  while switching to live delivery.
- **Task ownership.** Each ACP session owns its cancellation token, child
  process/process tree, and spawned tasks. No lock is held across `.await`.
- **Security defaults.** Preserve loopback Origin/CSRF checks, WS Origin
  checks, path/symlink containment, size caps, and bounded pairing rate
  limiting. LAN cleartext must remain an explicit insecure opt-in.

## Phase Gates

### Phase 0: De-risk and freeze the contract

- S-ARCH: resolve package layout, MSRV, current crate API choices, logging
  location, and release strategy.
- S-ACP-SPIKE: prove the current ACP Rust SDK against mock and a real agent:
  initialize, prompt streaming, file/shell callbacks, cancellation, auth, and
  MCP relay capability.
- S-CONTRACT: capture Go REST/WS/CLI/serialization fixtures and run a
  side-by-side differential harness using isolated data directories.

**Exit gate:** current SDK APIs are verified, the compatibility harness runs in
CI, and the Rust implementation has a published contract target before service
porting begins.

### Phase 1: Foundation and existing-state safety

- S-PATHUTIL and S-CONFIG start independently. S-INTERFACES follows
  S-PATHUTIL; S-EVENTS follows S-INTERFACES; S-MIGRATE follows S-CONFIG,
  S-EVENTS, and S-CONTRACT.
- Search DTOs move to shared types so interfaces do not depend on search.

**Exit gate:** existing Go-created state opens safely; typed shared models
serialize identically; durable event IDs and replay semantics are tested.

### Phase 2: Core host services

- S-SEARCH, S-FILES, S-SHELL, S-FSWATCH, S-UPLOADS, S-PERMISSIONS
- Each service adds its Go test inventory and property tests where appropriate
  (path handling, merge, and protocol parsing).

### Phase 3: Agent and workspace integration

- S-MCP, S-PAIRING, S-WORKSPACE
- S-ACP-CORE unblocks S-ACP-STREAM, S-ACP-CONTEXT, and S-ACP-PROVIDERS;
  S-ACP-AUTODETECT depends only on shared configuration and may run in parallel.

### Phase 4: Network surface and composition

- S-SYNC → S-SERVER → S-DAEMON → S-CLI

### Phase 5: Release validation

- S-BUILD, native Linux/macOS/Windows release CI, migration fixtures, contract
  suite, and real-agent E2E.

## Story Index

| Story | Title | Phase | Depends on |
|---|---|---:|---|
| [S-ARCH](stories/S-ARCH-architecture.md) | Architecture and dependency decisions | 0 | — |
| [S-ACP-SPIKE](stories/S-ACP-SPIKE-sdk-proof.md) | ACP SDK proof of capability | 0 | S-ARCH |
| [S-CONTRACT](stories/S-CONTRACT-compatibility.md) | Go/Rust contract differential harness | 0 | — |
| [S-PATHUTIL](stories/S-PATHUTIL-path-utils.md) | Path traversal and symlink utilities | 1 | — |
| [S-INTERFACES](stories/S-INTERFACES-traits.md) | Shared wire types, traits, and errors | 1 | S-PATHUTIL |
| [S-CONFIG](stories/S-CONFIG-config.md) | Config storage | 1 | — |
| [S-EVENTS](stories/S-EVENTS-event-store.md) | SQLite event store | 1 | S-INTERFACES |
| [S-MIGRATE](stories/S-MIGRATE-existing-state.md) | Existing state compatibility/migration | 1 | S-CONFIG, S-EVENTS, S-CONTRACT |
| [S-SEARCH](stories/S-SEARCH-search.md) | Workspace content search | 2 | S-INTERFACES |
| [S-FILES](stories/S-FILES-file-sync-merge.md) | Revision tracking and three-way merge | 2 | S-PATHUTIL, S-INTERFACES |
| [S-SHELL](stories/S-SHELL-shell-executor.md) | Workspace subprocess runner | 2 | S-PATHUTIL |
| [S-FSWATCH](stories/S-FSWATCH-file-watcher.md) | On-disk change detection | 2 | S-EVENTS |
| [S-UPLOADS](stories/S-UPLOADS-uploads.md) | File upload store | 2 | S-PATHUTIL |
| [S-PERMISSIONS](stories/S-PERMISSIONS-permissions.md) | Permission manager | 2 | S-EVENTS, S-INTERFACES |
| [S-MCP](stories/S-MCP-mcp-config.md) | MCP configuration and health | 3 | S-CONFIG |
| [S-PAIRING](stories/S-PAIRING-pairing-auth.md) | QR pairing and device auth | 3 | S-CONFIG, S-MIGRATE |
| [S-WORKSPACE](stories/S-WORKSPACE-workspace.md) | Workspace manager | 3 | S-FILES, S-SEARCH, S-PATHUTIL |
| [S-ACP-CORE](stories/S-ACP-CORE-session-transport.md) | ACP sessions and transport handlers | 3 | S-ACP-SPIKE, S-EVENTS, S-FILES, S-SHELL, S-PERMISSIONS |
| [S-ACP-STREAM](stories/S-ACP-STREAM-events.md) | ACP updates to ordered app events | 3 | S-ACP-CORE, S-CONTRACT |
| [S-ACP-CONTEXT](stories/S-ACP-CONTEXT-conversation.md) | Context, conversation, terminal, profiles | 3 | S-ACP-CORE, S-EVENTS, S-CONFIG |
| [S-ACP-PROVIDERS](stories/S-ACP-PROVIDERS-providers.md) | Provider management | 3 | S-ACP-CORE |
| [S-ACP-AUTODETECT](stories/S-ACP-AUTODETECT-registry.md) | Agent registry and autodetection | 3 | S-CONFIG |
| [S-SYNC](stories/S-SYNC-websocket-hub.md) | WebSocket sync hub | 4 | S-EVENTS, S-PAIRING |
| [S-SERVER](stories/S-SERVER-http-api.md) | HTTP, TLS, REST, and WS wiring | 4 | S-CONTRACT, S-MIGRATE, S-SYNC, all service stories |
| [S-DAEMON](stories/S-DAEMON-lifecycle.md) | Lifecycle and composition root | 4 | S-SERVER, all service stories |
| [S-CLI](stories/S-CLI-cli.md) | CLI commands | 4 | S-DAEMON |
| [S-BUILD](stories/S-BUILD-build-release.md) | Build, embed, and release | 5 | S-CLI |

## Superseded Story

`stories/S-ACP-acp-client.md` is retained as the source inventory and split-work
index only. It is not an implementation dependency; use the six ACP successor
stories in this epic instead.

## Verification Standards

A story is complete only when its mapped Go tests and any new contract tests
pass, `cargo fmt --check`, `cargo clippy -- -D warnings`, and the relevant
Rust tests pass. Phase gates additionally require the S-CONTRACT differential
suite. Final completion requires native release CI and the state-migration
fixtures to pass on all supported platforms.

## Deferred Optimization Candidates

After contract parity: SQLite read concurrency, durable permission policy and
audit records, and consolidation of conversation/device metadata into SQLite.
These are intentionally not part of the initial behavior-preserving port.
