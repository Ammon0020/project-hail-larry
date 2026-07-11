# ACP Client Temporal Coupling via Set* Methods

## Location
- [acp.go:137-174](file:///media/adam/extex/projects/project-hail-larry/internal/acp/acp.go#L137-L174) — `SetCallbacks`, `SetPipeline`, `SetEventStore`, `SetConversationTransfer`, `SetMcpConfigPath`, `SetStorePath`
- [daemon.go:200-220](file:///media/adam/extex/projects/project-hail-larry/internal/daemon/daemon.go#L200-L220) — call sites

## Problem

`acp.Client` is constructed with only 2 dependencies (`NewClient(wm, pm)`) but then requires **6 additional `Set*` calls** to become fully functional:

```go
acpClient := acp.NewClient(workspaceMgr, permissionMgr)
// ... many lines later ...
acpClient.SetPipeline(acp.NewPromptPipeline(...))
acpClient.SetEventStore(eventStore)
acpClient.SetConversationTransfer(conversationTransfer)
acpClient.SetMcpConfigPath(mcpConfigPath)
acpClient.SetStorePath(filepath.Join(cfg.DataDir, "conversations.json"))
acpClient.LoadConversations()
```

This is **temporal coupling** — the Client appears valid after `NewClient()` but will behave incorrectly (no middleware pipeline, no event persistence, no conversation transfer) until all `Set*` methods are called in the right order. There's no compile-time or runtime enforcement that these were called.

## Impact

- Forgetting a `Set*` call silently degrades behavior (e.g. no workspace context injection, no conversation persistence).
- Tests must remember to call the right subset of `Set*` methods for their scenario.
- The `Set*` pattern encourages mutable state after construction, making the Client harder to reason about.

## Suggested Fix

Use a constructor that takes all dependencies:

```go
type ClientConfig struct {
    WorkspaceMgr  interfaces.WorkspaceManager
    PermissionMgr interfaces.PermissionManager
    Pipeline      *PromptPipeline
    EventStore    interfaces.EventStore
    Transfer      *ConversationTransferMiddleware
    McpConfigPath string
    StorePath     string
    Callbacks     interfaces.ACPCallbacks  // optional
}

func NewClient(cfg ClientConfig) *Client { ... }
```

Dependencies that are genuinely optional can use `Option` values. Dependencies that are required become compile-time enforced by being struct fields.
