# Server Deps Uses Concrete Types Instead of Interfaces

## Location
- [server.go](file:///media/adam/extex/projects/project-hail-larry/internal/server/server.go#L33-L48) — `Deps` struct

## Problem

The `server.Deps` struct declares every dependency as a concrete type (`*events.Store`, `*pairing.Manager`, `*workspace.Manager`, `*acp.Client`, `*permissions.Manager`, `*sync.Hub`, `*acp.OpenFilesTracker`, `*uploads.Manager`) instead of using the interfaces already defined in `internal/interfaces/interfaces.go`.

The project has well-defined interfaces (`EventStore`, `WorkspaceManager`, `ACPClient`, `PermissionManager`) yet `Deps` bypasses all of them. This forces the `server` package to import every concrete implementation package, creating a tight coupling star-pattern where the server depends directly on `acp`, `events`, `pairing`, `permissions`, `workspace`, `sync`, and `uploads`.

## Impact

- **Testability:** Unit-testing server handlers requires spinning up real implementations (SQLite, file-system-backed workspaces) or reaching for `//go:build` tags, because the handlers reach through to concrete methods not on any interface.
- **Coupling:** The server package cannot be compiled without all implementation packages present. Adding or refactoring any implementation leaks into the server's import graph.
- **Architecture violation:** `AGENTS.md` says *"define interfaces, don't implement another lane's code"* — `Deps` currently bridges across every lane by importing concrete types.

## Suggested Fix

Replace concrete types in `Deps` with the existing interface types where they exist, and add slim interfaces for the remaining concrete types (`GetPending`, `SetCallback`, `Broadcast`, etc.) that the server actually uses:

```go
type Deps struct {
    EventStore    interfaces.EventStore
    PairingMgr    interfaces.PairingManager   // new interface
    WorkspaceMgr  interfaces.WorkspaceManager
    ACPClient     interfaces.ACPClient
    PermissionMgr interfaces.PermissionManager
    SyncHub       interfaces.SyncHub           // new interface
    // ...
}
```

This also means `GetPending()` and `SetCallback()` need to be added to `PermissionManager` (or a new extended interface), and a `SyncHub` interface with `Broadcast(Event)` / `HandleWS(...)` / `Shutdown()` should be defined.
