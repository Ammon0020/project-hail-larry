# Epic: Convert Go Backend to Rust

> **Status:** Planning. **Owner:** —. **Estimate:** see Difficulty section.
> Library references: `docs/rust-ecosystem/`. Stories: `stories/`.

## Goal

Replace the Go daemon (`cmd/app/` + `internal/`) with a Rust implementation
using official/well-maintained crates, keeping the React frontend and all
external behavior (REST API, WebSocket protocol, ACP integration, CLI
commands, file layout) identical. The frontend must not change; only the
backend binary and its build process change.

## Scope

**In scope:**
- All 17 `internal/` packages → Rust modules
- `cmd/app/` CLI → `clap` subcommands
- `cmd/mockagent/` test helper → Rust (or keep as Go for ACP testing)
- Build scripts (`build.sh` / `build.ps1`) → cargo-based
- All 37 Go test files → Rust tests

**Out of scope:**
- Frontend (`web/`) — unchanged, still `npm run build` → `dist/`
- ACP spec itself — we consume it via the Rust SDK
- New features — this is a port, not a rewrite of behavior

## Current Codebase Size

| Package | Go lines (non-test) | Story |
|---|---|---|
| `internal/acp/` | 5,066 | [S-ACP](stories/S-ACP-acp-client.md) |
| `internal/server/` | 2,382 | [S-SERVER](stories/S-SERVER-http-api.md) |
| `internal/pairing/` | 1,190 | [S-PAIRING](stories/S-PAIRING-pairing-auth.md) |
| `internal/daemon/` | 773 | [S-DAEMON](stories/S-DAEMON-lifecycle.md) |
| `internal/workspace/` | 750 | [S-WORKSPACE](stories/S-WORKSPACE-workspace.md) |
| `internal/mcp/` | 535 | [S-MCP](stories/S-MCP-mcp-config.md) |
| `internal/search/` | 437 | [S-SEARCH](stories/S-SEARCH-search.md) |
| `internal/interfaces/` | 434 | [S-INTERFACES](stories/S-INTERFACES-traits.md) |
| `internal/events/` | 395 | [S-EVENTS](stories/S-EVENTS-event-store.md) |
| `internal/permissions/` | 368 | [S-PERMISSIONS](stories/S-PERMISSIONS-permissions.md) |
| `internal/fswatch/` | 353 | [S-FSWATCH](stories/S-FSWATCH-file-watcher.md) |
| `internal/sync/` | 347 | [S-SYNC](stories/S-SYNC-websocket-hub.md) |
| `internal/shell/` | 320 | [S-SHELL](stories/S-SHELL-shell-executor.md) |
| `internal/config/` | 311 | [S-CONFIG](stories/S-CONFIG-config.md) |
| `internal/files/` | 254 | [S-FILES](stories/S-FILES-file-sync-merge.md) |
| `internal/uploads/` | 244 | [S-UPLOADS](stories/S-UPLOADS-uploads.md) |
| `internal/pathutil/` | 44 | [S-PATHUTIL](stories/S-PATHUTIL-path-utils.md) |
| `cmd/app/` | ~600 | [S-CLI](stories/S-CLI-cli.md) |
| **Total** | **~13,600 non-test** | |

## Difficulty Assessment: Moderate-Hard

**Overall: feasible and well-de-risked, but substantial.** This is not a
weekend project — it's a full backend rewrite of ~13,600 lines across 17
packages. However, several factors make it significantly easier than it
first appears:

### What makes it feasible (de-risked)

1. **Official Rust ACP SDK exists** (`agentclientprotocol/rust-sdk`). The
   single biggest risk — "is there a Rust ACP client SDK?" — is resolved.
   The Go code's heaviest package (`acp/`, 5,066 lines) has a direct
   official counterpart. See `docs/rust-ecosystem/acp-rust-sdk.md`.

2. **Clean interface boundaries already exist.** `internal/interfaces/`
   (434 lines) defines `EventStore`, `WorkspaceManager`, `ACPClient`,
   `PermissionManager`, `FileSync` as Go interfaces. These map 1:1 to Rust
   traits. The architecture is already "define interfaces, don't implement
   another lane's code" — exactly the trait-oriented design Rust wants.

3. **Every Go dependency has a mature Rust equivalent.** No exotic or
   niche libraries. See `docs/rust-ecosystem/README.md` mapping table —
   all "High" confidence.

4. **Frontend is untouched.** The React app talks REST + WebSocket over a
   stable JSON protocol. As long as the Rust server emits the same JSON
   shapes and routes, the frontend doesn't know or care what language the
   backend is.

5. **The codebase is well-tested.** 37 test files provide a behavioral
   spec to port against — each test documents expected behavior.

### What makes it hard (risk areas)

1. **ACP SDK API surface differs.** The Go SDK has the app implement a
   `Client` interface (callbacks). The Rust SDK uses a handler-registration
   pattern (`.on_receive_request()`). The transport layer
   (`transport.go`, 1,000+ lines) needs a rewrite, not a line-by-line port.
   **Risk: Medium.** Mitigation: the logic (translate ACP updates → events)
   is the same; only the wiring changes.

2. **Concurrency model shift.** Go's goroutine-per-request + channels →
   Tokio async tasks + `mpsc`/`broadcast` channels. This is well-trodden
   but requires care around `Send`/`Sync` bounds and not holding
   sync mutexes across `.await`. **Risk: Low-Medium.**

3. **CGO for SQLite.** The Go build uses pure-Go SQLite (`modernc`). Rust's
   `rusqlite` (bundled) compiles SQLite C source, requiring a C compiler.
   Acceptable for a self-hosted daemon but adds a build dependency.
   **Risk: Low.** Mitigation: `rusqlite` "bundled" feature handles it; just
   needs `cc` in the build environment.

4. **Platform-specific service install.** systemd/launchd/HKCU logic uses
   Go build tags → Rust `#[cfg(target_os)]`. Straightforward but tedious,
   and Windows service install may need a crate (`windows-service`).
   **Risk: Low.**

5. **Three-way merge** (`files.go`, 254 lines). The merge logic is
   algorithmic (diff3-style). Port carefully with tests. **Risk: Low.**

6. **Cross-compilation for releases.** Go cross-compiles trivially
   (`GOOS=windows go build`). Rust needs `rustup target add` + a cross
   linker (`cross` or `cargo-xwin` for Windows-from-Linux). **Risk: Low**
   (build tooling, not code).

## Execution Strategy

Port bottom-up by dependency layer, testing each layer before building on
it. Run Go and Rust daemons side-by-side during migration, comparing API
responses to catch behavioral drift.

### Phase 1: Foundation (no external I/O)
- S-PATHUTIL → S-INTERFACES → S-CONFIG → S-EVENTS

### Phase 2: Core services
- S-FILES → S-SHELL → S-SEARCH → S-FSWATCH → S-UPLOADS → S-PERMISSIONS

### Phase 3: External integration
- S-ACP (the big one) → S-MCP → S-PAIRING → S-WORKSPACE

### Phase 4: Server & wiring
- S-SYNC → S-SERVER → S-DAEMON → S-CLI

### Phase 5: Build & release
- Build scripts, cross-compilation, platform services, final E2E

## Story Index

| Story | Title | Phase | Depends on |
|---|---|---|---|
| [S-PATHUTIL](stories/S-PATHUTIL-path-utils.md) | Path traversal & symlink utils | 1 | — |
| [S-INTERFACES](stories/S-INTERFACES-traits.md) | Shared trait definitions | 1 | S-PATHUTIL |
| [S-CONFIG](stories/S-CONFIG-config.md) | Config storage (TOML) | 1 | — |
| [S-EVENTS](stories/S-EVENTS-event-store.md) | SQLite event store | 1 | S-INTERFACES |
| [S-FILES](stories/S-FILES-file-sync-merge.md) | Revision tracking + 3-way merge | 2 | S-PATHUTIL |
| [S-SHELL](stories/S-SHELL-shell-executor.md) | Workspace subprocess runner | 2 | S-PATHUTIL |
| [S-SEARCH](stories/S-SEARCH-search.md) | Workspace content search | 2 | — |
| [S-FSWATCH](stories/S-FSWATCH-file-watcher.md) | On-disk change detection | 2 | — |
| [S-UPLOADS](stories/S-UPLOADS-uploads.md) | File upload store | 2 | — |
| [S-PERMISSIONS](stories/S-PERMISSIONS-permissions.md) | Permission manager | 2 | S-EVENTS |
| [S-ACP](stories/S-ACP-acp-client.md) | ACP client (Rust SDK) | 3 | S-INTERFACES, S-SHELL, S-FILES, S-PERMISSIONS |
| [S-MCP](stories/S-MCP-mcp-config.md) | MCP config + health | 3 | — |
| [S-PAIRING](stories/S-PAIRING-pairing-auth.md) | QR + mnemonic pairing | 3 | S-CONFIG |
| [S-WORKSPACE](stories/S-WORKSPACE-workspace.md) | Workspace manager | 3 | S-FILES, S-SEARCH |
| [S-SYNC](stories/S-SYNC-websocket-hub.md) | WebSocket sync hub | 4 | S-EVENTS |
| [S-SERVER](stories/S-SERVER-http-api.md) | HTTP server + REST API | 4 | all above |
| [S-DAEMON](stories/S-DAEMON-lifecycle.md) | Daemon lifecycle + wiring | 4 | S-SERVER |
| [S-CLI](stories/S-CLI-cli.md) | CLI (clap) commands | 4 | S-DAEMON |
| [S-BUILD](stories/S-BUILD-build-release.md) | Build scripts, embed, cross-compile | 5 | S-CLI |

## Verification Per Story

Each story is "done" when:
1. Rust module compiles and passes `cargo test` for that module
2. `cargo clippy` is clean
3. Behavior matches the Go equivalent (verified via ported tests)
4. Story file updated with completion notes

## Open Questions

- [ ] Keep `cmd/mockagent/` as Go (for ACP conformance testing) or port too?
- [ ] Use `rusqlite` (sync + spawn_blocking) or `sqlx` (async) for SQLite?
- [ ] Does the Rust ACP SDK have the `mcp/message` relay (Go SDK gap)?
- [ ] Single crate or workspace with sub-crates per module?
