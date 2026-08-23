# Abstract Event System

## Scope
`acp` relies heavily on `SharedEventBus` (`crate::events::EventBus`) for persisting and broadcasting events. This ties ACP to the daemon's SQLite WAL implementation.

## Acceptance Criteria
- Define an `EventSink` trait in the ACP module specifying `append_and_publish` and `query` operations.
- Replace `SharedEventBus` across all `acp` configurations with `Arc<dyn EventSink>`.
- Implement `EventSink` for the daemon's `EventBus`.
