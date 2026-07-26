//! Narrow notification sink for new permission prompts.
//!
//! Replaces Go's post-construction `SetCallback` with a constructor-injected
//! dependency. The composition root wires [`EventBusPermissionSink`] (persist +
//! publish a `PermissionRequested` event through the [`EventBus`]); tests wire
//! [`NullSink`] or a capturing sink.
//!
//! [`EventBus`]: crate::events::EventBus

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::events::SharedEventBus;
use crate::interfaces::types::{Event, EventType, PermissionRequest};

/// Sink for broadcasting a newly-registered permission prompt.
///
/// Implementations persist (when applicable) and publish the prompt so paired
/// devices can render a decision UI. The trait is intentionally narrower than
/// [`crate::interfaces::EventPublisher`]: it accepts a typed
/// [`PermissionRequest`] and owns the event construction + durable-ID
/// assignment, so the manager does not depend on the event store shape.
#[async_trait]
pub trait PermissionSink: Send + Sync {
    /// Broadcast `req` to subscribers. Best-effort: failures are logged by the
    /// implementation and must not block the request flow.
    async fn broadcast_request(&self, req: &PermissionRequest);
}

/// No-op sink used when no event bus is wired (e.g. unit tests that only
/// exercise the policy / pending machinery).
pub struct NullSink;

#[async_trait]
impl PermissionSink for NullSink {
    async fn broadcast_request(&self, _req: &PermissionRequest) {}
}

/// Sink that persists + publishes a `PermissionRequested` event via the
/// [`EventBus`]. Append-then-publish ordering matches the rest of the daemon:
/// the durable ID is assigned before any live subscriber sees the prompt, so a
/// reconnecting device picks it up via replay.
///
/// [`EventBus`]: crate::events::EventBus
pub struct EventBusPermissionSink {
    bus: SharedEventBus,
}

impl EventBusPermissionSink {
    /// Wrap a shared event bus.
    #[must_use]
    pub fn new(bus: SharedEventBus) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl PermissionSink for EventBusPermissionSink {
    async fn broadcast_request(&self, req: &PermissionRequest) {
        // Build the flat wire event. ID 0 is assigned by the store on append.
        let mut event = Event::new(
            0,
            EventType::PermissionRequested,
            req.session_id.clone(),
            Utc::now(),
        );
        event.tool.clone_from(&req.tool);
        event.tool_kind.clone_from(&req.tool_kind);
        event.target.clone_from(&req.target);
        event.command.clone_from(&req.command);
        event.request_id.clone_from(&req.id);
        event.options = req.options.iter().map(|d| d.as_str().to_string()).collect();

        // Persist-before-publish; a store failure is logged but does not
        // propagate — the agent's Request call still blocks on a decision and
        // the stale sweeper eventually unblocks it.
        if let Err(e) = self.bus.append_and_publish(event).await {
            tracing::warn!(error = %e, "permission sink: failed to publish PermissionRequested");
        }
    }
}

/// Convenience: a sink that does nothing, boxed for the manager constructor.
#[must_use]
pub fn null_sink() -> Arc<dyn PermissionSink> {
    Arc::new(NullSink)
}
