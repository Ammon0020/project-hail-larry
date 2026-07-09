# ACPClient interface missing methods used by server (ListSessions, RegisterAgent, RemoveAgent, RenameSession, RebindSession)

- **Difficulty:** medium
- **Urgency:** low
- **File:** `internal/interfaces/interfaces.go`
- **Lines:** 200-225

## Description

The ACPClient interface (lines 200-225) declares only ListAgents, CreateSession, GetSessionInfo, SendPrompt, CancelSession, CloseSession. But the server calls s.deps.ACPClient.ListSessions (api.go:376), RegisterAgent (api.go:352), RemoveAgent (api.go:364), RenameSession (api.go:433), and RebindSession (api.go:445) — none of which are in the interface. The server's Deps.ACPClient is typed as *acp.Client (concrete, server.go:37), not interfaces.ACPClient, so this compiles, but it defeats the purpose of the shared interface contract: the interface claims to be 'the contract for communicating with AI agents' but is incomplete and would not admit the real client if anyone tried to program to the interface. This is a pre-existing gap, but the diff touches the interface (SendPrompt signature change) without fixing the drift, and the new SendPrompt signature is the kind of breaking change that should prompt a review of the whole contract.

## Recommendation

Either add the missing methods (ListSessions, RegisterAgent, RemoveAgent, RenameSession, RebindSession) to interfaces.ACPClient so it reflects the real contract, or change Deps.ACPClient to interfaces.ACPClient and add the missing methods. At minimum, add a comment noting the interface is a subset and the server uses the concrete type for the extended surface.

## Verification

grep'd ACPClient usages in internal/server/api.go: found ListSessions (376), RegisterAgent (352), RemoveAgent (364), RenameSession (433), RebindSession (445). None appear in interfaces.ACPClient (interfaces.go:202-225). Confirmed Deps.ACPClient is *acp.Client (server.go:37), not the interface.
