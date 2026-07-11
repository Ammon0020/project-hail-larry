# PermissionManager Interface Is Incomplete

## Location
- [interfaces.go:304-318](file:///media/adam/extex/projects/project-hail-larry/internal/interfaces/interfaces.go#L304-L318) — `PermissionManager` interface
- [permissions.go:276](file:///media/adam/extex/projects/project-hail-larry/internal/permissions/permissions.go#L276) — `GetPending()` method (not on interface)
- [permissions.go:110](file:///media/adam/extex/projects/project-hail-larry/internal/permissions/permissions.go#L110) — `SetCallback()` method (not on interface)
- [server/api.go:786](file:///media/adam/extex/projects/project-hail-larry/internal/server/api.go#L786) — uses `GetPending()` via concrete type
- [server/server.go:74](file:///media/adam/extex/projects/project-hail-larry/internal/server/server.go#L74) — uses `SetCallback()` via concrete type

## Problem

The `PermissionManager` interface defines only 3 methods: `Request`, `Respond`, `ClearSession`. But the server uses 2 additional methods through the concrete `*permissions.Manager` type:

- `GetPending() []PermissionRequest` — used in `handlePendingPermissions` and `handleRespondPermission`
- `SetCallback(fn func(PermissionRequest))` — used in `server.New()` to wire permission event broadcasting

Because these are not on the interface, `server.Deps` must use `*permissions.Manager` (concrete type), which defeats the purpose of the interface layer.

## Impact

- The server package is tightly coupled to `permissions.Manager` for these two methods.
- Any test that wants to mock the permission manager must either use the concrete type or add these methods to a test-local interface.

## Suggested Fix

Add `GetPending` and `SetCallback` (or `OnRequest`) to the `PermissionManager` interface:

```go
type PermissionManager interface {
    Request(ctx context.Context, req PermissionRequest) (PermissionDecision, error)
    Respond(ctx context.Context, requestID string, decision PermissionDecision) error
    ClearSession(sessionID string)
    GetPending() []PermissionRequest
    SetOnRequest(fn func(PermissionRequest))
}
```

This enables `server.Deps` to use `interfaces.PermissionManager` instead of `*permissions.Manager`.
