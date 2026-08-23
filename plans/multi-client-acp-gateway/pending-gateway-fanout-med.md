# S-GATEWAY-FANOUT — Multi-client event fan-out & session replay

## Outcome

Allow multiple client applications (Web, Mobile, Windsurf, Devin) to subscribe to the same active session event stream simultaneously and receive full state replay upon connecting.

## Work

1. Update daemon session event hub to maintain a set of active subscriber channels per `session_id`.
2. Broadcast incoming ACP `session/update` notifications (messages, thoughts, tool call states, plan changes) to all attached client channels in order.
3. On new client connection to an existing session, stream historical events from the SQLite WAL event store prior to attaching to live fan-out.
4. Track connected client presence and disconnect stale subscribers cleanly without affecting the underlying ACP agent session.

## Acceptance

- Multiple connected Web/IDE clients receive identical real-time streaming updates from a single agent session.
- A secondary client attaching mid-turn receives full session history replay and seamlessly transitions to live streaming.
- Client disconnects do not interrupt agent execution or invalidate session state.

## Verify

- Automated unit tests for session event broadcaster with multiple concurrent subscribers.
- Replay verification test comparing WAL history against live event stream.
- Integration test with mock agent streaming updates to 2+ subscribers.
