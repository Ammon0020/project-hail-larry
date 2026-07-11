# Daemon.New() Is a 300-Line Wiring Function

## Location
- [daemon.go:145-307](file:///media/adam/extex/projects/project-hail-larry/internal/daemon/daemon.go#L145-L307) — `New()` function

## Problem

`daemon.New()` is a **162-line function** that performs all dependency construction and wiring in a single procedural block:

1. Creates data directory
2. Opens SQLite event store
3. Constructs pairing manager + configures TTLs
4. Constructs workspace manager + loads persisted workspaces from config
5. Constructs permission manager
6. Constructs ACP client
7. Loads system messages
8. Creates open-files tracker
9. Creates conversation-transfer middleware
10. Builds the full middleware pipeline (6 middlewares)
11. Wires event store + transfer middleware + MCP config into ACP client
12. Loads conversations from disk
13. Creates sync hub
14. Autodetects agents, merges, verifies executables, registers
15. Creates uploads manager
16. Creates the HTTP server with all deps
17. Creates filesystem watcher + wires callbacks
18. Registers existing workspaces with the watcher

This is the classic "big-bang wiring" pattern. Every change to any subsystem requires reading and modifying this monolith.

## Impact

- No component can be initialized or tested in isolation — everything must go through this function.
- The ACP client wiring alone spans 7 `Set*` calls, which is a code smell (method injection after construction = temporal coupling).
- Adding a new subsystem means adding more lines to an already-long function.

## Suggested Fix

**Option A — Builder pattern:**
```go
func NewDaemon(cfg *Config) (*Daemon, error) {
    b := &daemonBuilder{cfg: cfg}
    return b.
        withEventStore().
        withPairing().
        withWorkspaces().
        withPermissions().
        withACP().
        withSync().
        withAgents().
        withUploads().
        withServer().
        withFSWatch().
        build()
}
```

**Option B — Functional options / config struct:** Pass a pre-constructed `SubsystemConfig` into `New()` so construction logic lives in focused functions.

Either approach makes each subsystem independently testable and reduces `New()` to a coordinator.
