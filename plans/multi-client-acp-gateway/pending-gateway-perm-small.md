# S-GATEWAY-PERM — Synchronized permission resolution & conflict handling

## Outcome

Ensure permission requests from the agent are broadcast to all connected clients and resolved idempotently across all attached frontends.

## Work

1. Broadcast `session/request_permission` prompt events to all active client subscribers for the session.
2. Implement first-wins decision handling in the daemon's permission service: the first valid user response (`allow_once`, `allow_always`, `reject_once`, `reject_always`) responds to the ACP agent.
3. Broadcast a `permission_resolved` event to all other clients so open permission prompts close or mark as resolved immediately across all UIs.
4. Gracefully reject duplicate or late permission responses from secondary clients with an informative status message.

## Acceptance

- Permission prompt appears on all attached devices/clients simultaneously.
- Deciding on one client resolves the prompt and updates all other connected UI screens instantly.
- Late or concurrent responses from another client are handled without error or duplicate ACP response.

## Verify

- Unit tests for race conditions with concurrent permission decision inputs.
- Integration test checking multi-client WS event emission and prompt cancellation broadcasting.
