# S-GATEWAY-ADAPTER — External IDE adapter & session attach API

## Outcome

Expose endpoints and adapters enabling external IDE tools like Windsurf, Devin, or VS Code extensions to discover, attach to, and send prompts into active daemon ACP sessions.

## Work

1. Expose REST/WS endpoints for listing active sessions, discovering agent models, and attaching as an external IDE client.
2. Implement prompt input serialization so prompts sent from an external IDE client are queued into the session turn loop identically to web UI prompts.
3. Include client attribution on user prompt events (e.g., indicating whether a prompt originated from Windsurf, Devin, or Web UI).
4. Provide documentation and example client integration snippet for external IDE extensions.

## Acceptance

- External clients can query `/api/sessions`, attach via WebSocket, and submit prompts.
- Prompts from external clients execute through the daemon's existing prompt pipeline and produce streamed events to all subscribers.
- UI correctly attributes prompt origins when displaying conversation history.

## Verify

- End-to-end API tests connecting an external client mock, attaching to a session, sending a prompt, and verifying responses across subscribers.
- Contract tests for session list, attach, and prompt endpoints.
