# Daemon Imports Concrete ACP Types for Agent Management

## Location
- [daemon.go:17](file:///media/adam/extex/projects/project-hail-larry/internal/daemon/daemon.go#L17) — imports `acp` package
- [daemon.go:90-125](file:///media/adam/extex/projects/project-hail-larry/internal/daemon/daemon.go#L90-L125) — `mergeAutodetectedAgents()` uses `acp.AgentInfo`
- [daemon.go:226-243](file:///media/adam/extex/projects/project-hail-larry/internal/daemon/daemon.go#L226-L243) — agent verification + registration loop

## Problem

The daemon imports `internal/acp` to use `acp.AgentInfo`, `acp.Autodetect()`, `acp.NewClient()`, and multiple ACP middleware constructors. This creates a direct dependency from the daemon (lifecycle coordinator) to the ACP implementation package.

The `interfaces` package already defines `interfaces.AgentInfo` and `interfaces.AgentModel`, but the daemon uses `acp.AgentInfo` instead. The autodetection and agent-merging logic (`mergeAutodetectedAgents`) operates on `acp.AgentInfo` when it should operate on the interface type.

## Impact

- The daemon cannot be tested without the full ACP implementation package.
- Agent management logic (autodetect, merge, executable verification) is embedded in the daemon when it could be a standalone concern.

## Suggested Fix

1. Use `interfaces.AgentInfo` in `mergeAutodetectedAgents()` and the registration loop.
2. Move autodetection behind an interface: `AgentDiscoverer` with `Discover() []interfaces.AgentInfo`.
3. Construct the ACP client via a factory function or the `interfaces.ACPClient` interface to reduce the daemon's dependency on the concrete `acp` package.

Note: Some daemon→acp coupling is unavoidable (middleware construction). A factory pattern (`acp.NewClientFromConfig(cfg)`) would encapsulate that.
