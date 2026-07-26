use chrono::Utc;

use super::handlers::HandlerDeps;
use agent_client_protocol::schema::v1::SessionNotification;

use crate::acp::stream;
use crate::events::SharedEventBus;
use crate::interfaces::{wire::typed_event_to_wire, AppError, EventMeta, EventPayload, TypedEvent};

/// Translate, persist, and publish an inbound ACP update in receive order.
///
/// The SDK dispatches notifications in stream order. Awaiting the durable
/// append here keeps that order through `SQLite` before subscribers observe it.
pub(super) async fn handle_session_notification(
    deps: &HandlerDeps,
    notification: SessionNotification,
) -> Result<(), AppError> {
    let Some(payload) = stream::session_update_to_payload(&notification.update) else {
        return Ok(());
    };
    append_payload(&deps.event_bus, &deps.local_session_id, payload).await
}

/// Project a typed event through the only public wire adapter, then persist it
/// before broadcasting to live listeners. An ID of zero requests `SQLite`'s
/// autoincrement assignment and is replaced before publication.
pub(super) async fn append_payload(
    event_bus: &SharedEventBus,
    session_id: &str,
    payload: EventPayload,
) -> Result<(), AppError> {
    let typed = TypedEvent {
        meta: EventMeta {
            id: 0,
            session_id: session_id.to_string(),
            timestamp: Utc::now(),
        },
        payload,
    };
    event_bus
        .append_and_publish(typed_event_to_wire(&typed))
        .await?;
    Ok(())
}
