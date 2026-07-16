//! Durable event bus: persist-before-publish + reconnect handoff.
//!
//! [`EventBus`] owns an [`Store`] and a broadcast channel. Callers that need
//! durable publication use [`EventBus::append_and_publish`]: the event is written
//! to SQLite and only then sent to live subscribers. That guarantees
//! persist-before-publish ordering.
//!
//! Reconnection follows subscribe → replay from durable cursor → deduplicate by
//! ID → switch to live delivery ([`EventBus::subscribe`]).

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;
use tracing::warn;

use super::store::Store;
use crate::interfaces::{AppError, Event, EventPublisher, EventStore};

/// Capacity of the live broadcast channel.
///
/// Large enough for a burst of stream updates without forcing slow consumers to
/// miss events under normal load; lagging receivers get a lagged error and must
/// re-subscribe with a durable cursor.
const BROADCAST_CAPACITY: usize = 1024;

/// Event store + live publisher with reconnect-friendly subscription.
///
/// Implements both [`EventStore`] (delegates to the inner store) and
/// [`EventPublisher`] (broadcasts already-persisted events to live subscribers).
#[derive(Clone)]
pub struct EventBus {
    store: Store,
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    /// Create a bus wrapping an existing store.
    pub fn new(store: Store) -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self { store, tx }
    }

    /// Open a store at `db_path` and wrap it in a bus.
    pub fn open(db_path: impl AsRef<std::path::Path>) -> Result<Self, AppError> {
        Ok(Self::new(Store::open(db_path)?))
    }

    /// Borrow the underlying durable store (prune, count, pragmas, …).
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Persist then publish: durable ID is assigned before any subscriber sees
    /// the event. This is the preferred write path for production callers.
    ///
    /// # Errors
    /// Propagates store append failures. Publish is best-effort after a durable
    /// write (no live subscribers is not an error).
    pub async fn append_and_publish(&self, event: Event) -> Result<Event, AppError> {
        let stored = self.store.append(event).await?;
        // Persist-before-publish: only notify after the durable write returned.
        self.publish_live(&stored);
        Ok(stored)
    }

    /// Broadcast an already-persisted event to live subscribers.
    ///
    /// No-op (Ok) when there are zero receivers — matches typical hub semantics.
    fn publish_live(&self, event: &Event) {
        match self.tx.send(event.clone()) {
            Ok(n) => {
                tracing::trace!(id = event.id, receivers = n, "event published live");
            }
            Err(_) => {
                // Zero receivers: still durable; live clients will pick up via replay.
            }
        }
    }

    /// Subscribe for reconnect handoff.
    ///
    /// 1. Snapshot the live receiver (future events).
    /// 2. Replay durable events with `id > after_id`.
    /// 3. Deliver live events, skipping any already seen during replay
    ///    (dedupe by ID).
    ///
    /// Returns a receiver handle that yields events in order. Dropping the
    /// handle unsubscribes.
    pub async fn subscribe(&self, after_id: i64) -> Result<EventSubscription, AppError> {
        // Register for live delivery first so we cannot miss events that land
        // between replay and subscription.
        let live_rx = self.tx.subscribe();

        // Replay durable history from the caller's cursor.
        let replay = self.store.query_all(after_id, 0).await?;
        let mut last_replay_id = after_id;
        for e in &replay {
            if e.id > last_replay_id {
                last_replay_id = e.id;
            }
        }

        Ok(EventSubscription {
            replay,
            replay_idx: 0,
            last_seen_id: last_replay_id,
            live_rx,
        })
    }
}

/// Subscription producing replayed then live events with ID-based dedupe.
pub struct EventSubscription {
    replay: Vec<Event>,
    replay_idx: usize,
    /// Highest event ID already delivered (replay or live). Live events with
    /// `id <= last_seen_id` are skipped as duplicates.
    last_seen_id: i64,
    live_rx: broadcast::Receiver<Event>,
}

impl EventSubscription {
    /// Next event for this subscriber, or `None` when the bus is closed and
    /// replay is exhausted.
    ///
    /// Replay is drained first; then live events are read with deduplication.
    pub async fn recv(&mut self) -> Option<Event> {
        // Drain replay buffer first (subscribe → replay).
        if self.replay_idx < self.replay.len() {
            let event = self.replay[self.replay_idx].clone();
            self.replay_idx += 1;
            if event.id > self.last_seen_id {
                self.last_seen_id = event.id;
            }
            return Some(event);
        }
        // Free replay memory once drained.
        if !self.replay.is_empty() {
            self.replay.clear();
            self.replay.shrink_to_fit();
        }

        // Live delivery with dedupe by ID.
        loop {
            match self.live_rx.recv().await {
                Ok(event) => {
                    if event.id <= self.last_seen_id {
                        // Already delivered via replay (race window).
                        continue;
                    }
                    self.last_seen_id = event.id;
                    return Some(event);
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    // Slow consumer: log and keep reading. Callers that need a
                    // strict cursor should re-subscribe with last_seen_id.
                    warn!(skipped = n, "event subscription lagged; skipping to latest");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    /// Highest durable ID delivered so far (for reconnect cursors).
    pub fn last_seen_id(&self) -> i64 {
        self.last_seen_id
    }
}

#[async_trait]
impl EventStore for EventBus {
    async fn append(&self, event: Event) -> Result<Event, AppError> {
        // Trait-level append is durable only — does not auto-publish — so
        // callers can stage events. Prefer `append_and_publish` for the full
        // persist-before-publish path.
        self.store.append(event).await
    }

    async fn query(
        &self,
        session_id: &str,
        after_id: i64,
        limit: i32,
    ) -> Result<Vec<Event>, AppError> {
        self.store.query(session_id, after_id, limit).await
    }

    async fn query_all(&self, after_id: i64, limit: i32) -> Result<Vec<Event>, AppError> {
        self.store.query_all(after_id, limit).await
    }
}

#[async_trait]
impl EventPublisher for EventBus {
    /// Publish a previously-persisted event to live subscribers.
    ///
    /// Callers must assign the durable ID (via [`EventStore::append`]) before
    /// calling this. Prefer [`EventBus::append_and_publish`] when both steps
    /// are needed together.
    async fn publish(&self, event: &Event) -> Result<(), AppError> {
        if event.id == 0 {
            return Err(AppError::validation(
                "publish requires a durable event id (append first)",
            ));
        }
        self.publish_live(event);
        Ok(())
    }
}

/// Shared bus handle type used by composition roots.
pub type SharedEventBus = Arc<EventBus>;
